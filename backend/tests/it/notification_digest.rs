//! The notification digest goes out once per user and workspace, under that
//! workspace's name and security note, listing only its own notifications.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use uuid::Uuid;

use backend::models::{NewNotification, UpdateSiteSettings};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

fn pinned<T>(
    conn: &mut backend::db::DbConnection,
    workspace_id: i32,
    f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>,
) -> T {
    let actor = ActorContext::system("test:digest").with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, f).expect("pinned write")
}

fn brand(conn: &mut backend::db::DbConnection, workspace_id: i32, app_name: &str) {
    pinned(conn, workspace_id, |c| {
        backend::repository::site_settings::update_site_settings(
            c,
            UpdateSiteSettings {
                app_name: Some(app_name.to_string()),
                email_security_note_enabled: Some(true),
                email_security_note_template: Some(Some(
                    "Mail from {{brand_name}} never asks for your password.".to_string(),
                )),
                ..Default::default()
            },
        )
    });
}

fn notify(
    conn: &mut backend::db::DbConnection,
    workspace_id: i32,
    user: Uuid,
    type_id: i32,
    title: &str,
) {
    use backend::schema::notifications;
    pinned(conn, workspace_id, |c| {
        diesel::insert_into(notifications::table)
            .values(NewNotification {
                uuid: Uuid::now_v7(),
                user_uuid: user,
                notification_type_id: type_id,
                entity_type: "ticket".to_string(),
                entity_id: 1,
                title: title.to_string(),
                body: None,
                metadata: None,
                channels_delivered: serde_json::json!([]),
                interrupts: true,
                source_sync_id: None,
            })
            .execute(c)
    });
}

#[actix_web::test]
async fn each_workspace_sends_its_own_digest_with_its_own_note() {
    use backend::schema::{notification_preferences, notification_types, outbound_emails};
    use backend::schema::{notifications, user_emails};

    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = crate::common::seed_two_workspaces(&mut conn);
    let (a, b) = (ws.a.workspace_id, ws.b.workspace_id);
    let user = ws.a.member_uuid;

    diesel::insert_into(user_emails::table)
        .values((
            user_emails::user_uuid.eq(user),
            user_emails::email.eq("digest@example.com"),
            user_emails::email_type.eq("personal"),
            user_emails::is_primary.eq(true),
            user_emails::is_verified.eq(true),
        ))
        .execute(&mut conn)
        .expect("insert email");
    let type_id: i32 = notification_types::table
        .select(notification_types::id)
        .order(notification_types::id)
        .first(&mut conn)
        .expect("a seeded notification type");
    diesel::insert_into(notification_preferences::table)
        .values((
            notification_preferences::user_uuid.eq(user),
            notification_preferences::notification_type_id.eq(type_id),
            notification_preferences::channel.eq("email"),
            notification_preferences::enabled.eq(true),
            notification_preferences::frequency.eq(Some("digest")),
            notification_preferences::workspace_id.eq(a),
        ))
        .execute(&mut conn)
        .expect("digest preference");

    brand(&mut conn, a, "Alpha Desk");
    brand(&mut conn, b, "Beta Desk");
    // B's people use its own domain; A has none, so its links use FRONTEND_URL.
    {
        use backend::schema::workspaces;
        diesel::update(workspaces::table.find(b))
            .set(workspaces::custom_domain.eq(Some("help.beta.example")))
            .execute(&mut conn)
            .expect("custom domain");
    }
    notify(&mut conn, a, user, type_id, "Printer on fire");
    notify(&mut conn, b, user, type_id, "VPN down");
    drop(conn);

    backend::services::scheduled_jobs::send_notification_digests(
        pool.clone(),
        "https://desk.example.com".to_string(),
    )
    .await
    .expect("digest run");

    let mut conn = pool.get().expect("conn");
    let mut sent: Vec<(i32, String, String)> = outbound_emails::table
        .filter(outbound_emails::recipient.eq("digest@example.com"))
        .select((
            outbound_emails::workspace_id,
            outbound_emails::subject,
            outbound_emails::body_text,
        ))
        .order(outbound_emails::workspace_id)
        .load(&mut conn)
        .expect("queued digests");
    assert_eq!(sent.len(), 2, "one digest per workspace: {sent:?}");

    let (beta_ws, beta_subject, beta_body) = sent.pop().expect("beta");
    let (alpha_ws, alpha_subject, alpha_body) = sent.pop().expect("alpha");
    assert_eq!((alpha_ws, beta_ws), (a, b));

    assert!(alpha_subject.contains("Alpha Desk"), "{alpha_subject}");
    assert!(alpha_body.contains("Printer on fire"), "{alpha_body}");
    assert!(
        alpha_body.contains("View them: https://desk.example.com"),
        "{alpha_body}"
    );
    assert!(
        alpha_body.ends_with("Mail from Alpha Desk never asks for your password."),
        "{alpha_body}"
    );
    assert!(beta_subject.contains("Beta Desk"), "{beta_subject}");
    assert!(beta_body.contains("VPN down"), "{beta_body}");
    assert!(
        beta_body.contains("View them: https://help.beta.example"),
        "the link where B's other email to them points: {beta_body}"
    );
    assert!(
        beta_body.ends_with("Mail from Beta Desk never asks for your password."),
        "{beta_body}"
    );
    for text in [&alpha_subject, &alpha_body] {
        assert!(!text.contains("Beta Desk") && !text.contains("VPN down"));
    }
    for text in [&beta_subject, &beta_body] {
        assert!(!text.contains("Alpha Desk") && !text.contains("Printer on fire"));
    }

    // Both notifications are marked sent, so the next run sends nothing.
    let undelivered: i64 = notifications::table
        .filter(notifications::user_uuid.eq(user))
        .filter(diesel::dsl::not(
            notifications::channels_delivered.contains(serde_json::json!(["email"])),
        ))
        .count()
        .get_result(&mut conn)
        .expect("count");
    assert_eq!(undelivered, 0);
    drop(conn);
    backend::services::scheduled_jobs::send_notification_digests(
        pool.clone(),
        "https://desk.example.com".to_string(),
    )
    .await
    .expect("second run");
    let mut conn = pool.get().expect("conn");
    let total: i64 = outbound_emails::table
        .filter(outbound_emails::recipient.eq("digest@example.com"))
        .count()
        .get_result(&mut conn)
        .expect("count");
    assert_eq!(total, 2, "a second run sends nothing new");
}
