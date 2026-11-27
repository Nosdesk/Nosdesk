//! What holds the request form's confirmation on a self-hosted deployment:
//! the suppressions the address or the workspace chose, and the per-address
//! cap. Not a hard bounce, which a forged DSN can add, and not the ceiling on
//! new addresses, which is for mail under a hosted platform's own sender.
//! Self-hosted, the workspace's own branding and names stay in the mail.

use backend::handlers::users::{confirmation_hold, NEW_ADDRESS_CONFIRMATIONS_PER_DAY};
use backend::models::{email_suppression_reason, NewEmailSuppression, NewOutboundEmail};
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestDb, TestPool};

const REF: &str = "test:guest_confirmation_holds";

fn desk() -> (TestDb, TestPool, i32) {
    common::ensure_test_keyring();
    let db = TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(4);
    (db, pool, seeded.a.workspace_id)
}

fn confirmation_to(pool: &TestPool, ws: i32, to: &str, n: usize) {
    run_in_workspace(pool, REF, ws, |c| {
        backend::repository::outbound_emails::enqueue(
            c,
            NewOutboundEmail {
                channel_id: None,
                ticket_id: None,
                comment_id: None,
                recipient: to.into(),
                subject: "Confirm".into(),
                body_text: "Confirm".into(),
                body_html: None,
                message_id: format!("hold-{ws}-{n}@nosdesk.test"),
                in_reply_to: None,
                references_list: vec![],
                headers_json: serde_json::json!({}),
                correlation_id: None,
                idempotency_key: Some(format!(
                    "{}hold-{n}",
                    backend::services::transactional_email::GUEST_CONFIRMATION_KEY_PREFIX
                )),
                sender_identity: backend::models::outbound_email_sender_identity::PLATFORM.into(),
                mail_class: backend::models::outbound_email_mail_class::TRANSACTIONAL.into(),
            },
        )
    })
    .expect("seed confirmation");
}

fn hold(pool: &TestPool, ws: i32, to: &str) -> Option<&'static str> {
    run_in_workspace(pool, REF, ws, |c| Ok(confirmation_hold(c, to, ws))).expect("hold")
}

/// A stop link, an admin's entry or a complaint holds the confirmation; a
/// hard bounce doesn't.
#[test]
fn only_the_suppressions_an_address_or_workspace_chose_hold_a_confirmation() {
    let (_db, pool, ws) = desk();
    let suppress = |to: &str, reason: &str| {
        run_in_workspace(&pool, REF, ws, |c| {
            backend::repository::email_suppressions::upsert(
                c,
                NewEmailSuppression {
                    email: to.into(),
                    reason: reason.into(),
                    bounce_diagnostic: None,
                    workspace_id: ws,
                },
            )
        })
        .expect("suppress");
    };
    for reason in [
        email_suppression_reason::REQUESTED,
        email_suppression_reason::MANUAL,
        email_suppression_reason::COMPLAINT,
    ] {
        let to = format!("{reason}@example.com");
        suppress(&to, reason);
        assert_eq!(hold(&pool, ws, &to), Some("suppressed"), "{reason}");
    }
    suppress("bounced@example.com", email_suppression_reason::HARD_BOUNCE);
    assert_eq!(hold(&pool, ws, "bounced@example.com"), None);
}

/// Self-hosted, a workspace may confirm as many new addresses as its form is
/// given; the cap of three a day per address still holds.
#[test]
fn self_hosted_has_no_ceiling_on_new_addresses() {
    let (_db, pool, ws) = desk();
    for n in 0..NEW_ADDRESS_CONFIRMATIONS_PER_DAY as usize {
        confirmation_to(&pool, ws, &format!("stranger{n}@example.com"), n);
    }
    assert_eq!(hold(&pool, ws, "brand-new@example.com"), None);
    for n in 0..3 {
        confirmation_to(&pool, ws, "Again@Example.com", 1000 + n);
    }
    assert_eq!(hold(&pool, ws, "again@example.com"), Some("per_address"));
}

/// Self-hosted, the confirmation, an invitation and a reset keep the
/// workspace's branding, its security note and the names, whether or not the
/// address has proven itself.
#[test]
fn self_hosted_mail_keeps_the_workspaces_branding_and_names() {
    let (_db, pool, ws) = desk();
    run_in_workspace(&pool, REF, ws, |c| {
        backend::repository::site_settings::update_site_settings(
            c,
            backend::models::UpdateSiteSettings {
                app_name: Some("Acme Support Desk With A Very Long Name Indeed".into()),
                ..Default::default()
            },
        )
    })
    .expect("settings");
    let (mail, confirmation) = run_in_workspace(&pool, REF, ws, |c| {
        Ok((
            backend::utils::email_branding::instance_mail_for(
                c,
                "https://desk.example.com",
                "nobody@example.com",
            ),
            backend::utils::email_branding::guest_confirmation_branding(
                c,
                "https://desk.example.com",
            ),
        ))
    })
    .expect("branding");
    assert!(mail.names);
    for branding in [&mail.branding, &confirmation] {
        assert_eq!(
            branding.app_name,
            "Acme Support Desk With A Very Long Name Indeed"
        );
    }
}

/// A stop link after a hard bounce replaces the reason and keeps the bounce's
/// count and diagnostic.
#[test]
fn a_stop_after_a_bounce_keeps_the_bounce_record() {
    let (_db, pool, ws) = desk();
    let suppress = |reason: &str, diagnostic: Option<&str>| {
        run_in_workspace(&pool, REF, ws, |c| {
            backend::repository::email_suppressions::upsert(
                c,
                NewEmailSuppression {
                    email: "gone@example.com".into(),
                    reason: reason.into(),
                    bounce_diagnostic: diagnostic.map(str::to_string),
                    workspace_id: ws,
                },
            )
        })
        .expect("suppress")
    };
    let bounced = suppress(
        email_suppression_reason::HARD_BOUNCE,
        Some("550 no such user"),
    );
    let row = suppress(email_suppression_reason::REQUESTED, None);
    assert_eq!(row.reason, email_suppression_reason::REQUESTED);
    assert_eq!(row.bounce_count, bounced.bounce_count);
    assert_eq!(row.bounce_diagnostic.as_deref(), Some("550 no such user"));
}
