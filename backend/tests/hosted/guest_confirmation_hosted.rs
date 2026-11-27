//! The request form's confirmation email on Cloud: a stop link that works, a
//! per-address cap counted across every workspace, a workspace named only in
//! a short plain slot, and a daily ceiling on addresses a workspace has never
//! mailed. The emails are asserted on their outbound queue rows; nothing is
//! sent.
//!
//! Part of the hosted binary: building the confirmation's links needs a
//! workspace origin, which `NOSDESK_TENANT_DOMAIN` gives.

use actix_web::dev::Service as _;
use actix_web::{web, App, HttpMessage as _};
use diesel::prelude::*;

use backend::handlers::users::{send_guest_ticket_confirmation, SendInvitationResult};
use backend::models::OutboundEmail;
use backend::services::transactional_email::GUEST_CONFIRMATION_KEY_PREFIX;
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_context};

use crate::common::{self, TestDb, TestPool, TwoWorkspaces};

const REF: &str = "test:guest_confirmation_hosted";

struct Cloud {
    db: TestDb,
    /// Production's role: row security applies.
    pool: TestPool,
    seeded: TwoWorkspaces,
}

fn cloud() -> Cloud {
    crate::hosted_env();
    common::ensure_test_keyring();
    let db = TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(4);
    Cloud { db, pool, seeded }
}

impl Cloud {
    fn context(&self, ws: i32) -> backend::extractors::WorkspaceContext {
        let w = run_in_workspace(&self.pool, REF, ws, |c| {
            backend::repository::workspaces::find_by_id(c, ws)
        })
        .expect("workspace")
        .expect("exists");
        backend::extractors::WorkspaceContext {
            workspace_id: w.id,
            workspace_uuid: w.uuid,
            slug: w.slug,
            name: w.name,
            custom_domain: w.custom_domain,
            organisation_id: w.organisation_id,
        }
    }

    /// A guest account for `email`, as the request form makes one.
    fn guest(&self, ws: i32, email: &str) -> uuid::Uuid {
        run_in_workspace(&self.pool, REF, ws, |c| {
            match backend::repository::user_helpers::find_or_create_guest_user(
                email, "Sam", c, None,
            )? {
                backend::repository::user_helpers::GuestUserResult::Created(u)
                | backend::repository::user_helpers::GuestUserResult::Existing(u) => Ok(u.uuid),
                _ => panic!("expected a guest"),
            }
        })
        .expect("guest")
    }

    /// Send `ws`'s confirmation to `email`, as the request form does.
    fn confirm(&self, ws: i32, email: &str) -> SendInvitationResult {
        let user = self.guest(ws, email);
        let http = actix_web::test::TestRequest::default().to_http_request();
        http.extensions_mut().insert(self.context(ws));
        let actor = ActorContext::system("guest:ticket_create").with_workspace(ws);
        with_actor_context::<_, diesel::result::Error>(
            &mut self.pool.get().expect("conn"),
            &actor,
            |c| Ok(send_guest_ticket_confirmation(c, &http, user, email, ws)),
        )
        .expect("send")
    }

    /// Every confirmation queued to `email`, in any workspace, oldest first.
    fn confirmations(&self, email: &str) -> Vec<OutboundEmail> {
        use backend::schema::outbound_emails as o;
        o::table
            .filter(o::recipient.eq(email))
            .filter(o::idempotency_key.like(format!("{GUEST_CONFIRMATION_KEY_PREFIX}%")))
            .order(o::id)
            .load(&mut self.db.conn())
            .expect("confirmations")
    }

    /// `count` confirmations queued in `ws` today, each the first mail to a
    /// new address.
    fn first_contacts(&self, ws: i32, count: usize) {
        let mut c = self.db.conn();
        for i in 0..count {
            diesel::insert_into(backend::schema::outbound_emails::table)
                .values((
                    backend::schema::outbound_emails::workspace_id.eq(ws),
                    backend::schema::outbound_emails::recipient
                        .eq(format!("stranger{i}@elsewhere.example")),
                    backend::schema::outbound_emails::subject.eq("Confirm"),
                    backend::schema::outbound_emails::body_text.eq("Confirm"),
                    backend::schema::outbound_emails::message_id
                        .eq(format!("seed-{ws}-{i}@nosdesk.test")),
                    backend::schema::outbound_emails::idempotency_key
                        .eq(format!("{GUEST_CONFIRMATION_KEY_PREFIX}seed-{i}")),
                    backend::schema::outbound_emails::status.eq("sent"),
                ))
                .execute(&mut c)
                .expect("seed confirmation");
        }
    }
}

fn sent(result: &SendInvitationResult) -> bool {
    matches!(result, SendInvitationResult::Success)
}

/// The confirmation's stop link, and the `List-Unsubscribe` one-click POST
/// it is, puts the address on the workspace's suppression list, and the
/// workspace sends it no more confirmations. Another workspace still may.
#[actix_web::test]
async fn the_stop_link_stops_the_confirmations() {
    let c = cloud();
    let (a, b) = (c.seeded.a.workspace_id, c.seeded.b.workspace_id);
    let to = "Stop.Me@victim.example";
    assert!(sent(&c.confirm(a, to)));
    let first = &c.confirmations(to)[0];
    let url = first.headers_json["List-Unsubscribe"]
        .as_str()
        .expect("List-Unsubscribe")
        .to_string();
    assert!(first.body_text.contains(&url), "the body links it too");
    let path = &url[url.find("/api/").expect("a path")..];

    let ctx = c.context(a);
    let app = actix_web::test::init_service(
        App::new()
            .app_data(web::Data::new(c.pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ctx.clone());
                srv.call(req)
            })
            .service(web::scope("/api/public").configure(backend::handlers::email_stop::config)),
    )
    .await;
    // A mail client's RFC 8058 one-click.
    let resp = actix_web::test::call_service(
        &app,
        actix_web::test::TestRequest::post()
            .uri(path)
            .insert_header(("Content-Type", "application/x-www-form-urlencoded"))
            .set_payload("List-Unsubscribe=One-Click")
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), 200);

    assert!(matches!(c.confirm(a, to), SendInvitationResult::Capped));
    assert_eq!(c.confirmations(to).len(), 1, "nothing more queued in A");
    assert!(sent(&c.confirm(b, to)), "B's list is its own");
}
/// The cap of three a day per address holds across workspaces: two from A
/// and one from B use it up, and B's next is held.
#[actix_web::test]
async fn the_daily_cap_on_one_address_counts_every_workspace() {
    let c = cloud();
    let (a, b) = (c.seeded.a.workspace_id, c.seeded.b.workspace_id);
    let to = "target@victim.example";
    assert!(sent(&c.confirm(a, to)));
    assert!(sent(&c.confirm(a, to)));
    assert!(sent(&c.confirm(b, to)));
    assert!(matches!(c.confirm(b, to), SendInvitationResult::Capped));
    assert!(matches!(c.confirm(a, to), SendInvitationResult::Capped));
    assert_eq!(c.confirmations(to).len(), 3);
}

/// Under the platform's sender, the workspace is its name alone: plain text,
/// one line, at most 40 characters. No logo, and no security note it wrote.
#[actix_web::test]
async fn the_confirmation_names_the_workspace_in_a_short_plain_slot() {
    let c = cloud();
    let a = c.seeded.a.workspace_id;
    let long = "Acme Support\nURGENT: verify your account at evil.example today";
    diesel::update(backend::schema::workspaces::table.find(a))
        .set(backend::schema::workspaces::name.eq("Workspace Row Name"))
        .execute(&mut c.db.conn())
        .expect("rename");
    run_in_workspace(&c.pool, REF, a, |conn| {
        backend::repository::site_settings::update_site_settings(
            conn,
            backend::models::UpdateSiteSettings {
                app_name: Some(long.into()),
                email_security_note_enabled: Some(true),
                email_security_note_template: Some(Some("TENANT NOTE {{domain}}".into())),
                ..Default::default()
            },
        )
    })
    .expect("settings");
    diesel::update(backend::schema::site_settings::table)
        .filter(backend::schema::site_settings::workspace_id.eq(a))
        .set(
            backend::schema::site_settings::email_logo.eq(Some(serde_json::json!({
                "url": "/uploads/logo.png", "width": 120, "height": 40,
                "reads_on_light": true, "reads_on_dark": true
            }))),
        )
        .execute(&mut c.db.conn())
        .expect("logo");

    let to = "slot@victim.example";
    assert!(sent(&c.confirm(a, to)));
    let m = &c.confirmations(to)[0];
    let slot = "Acme Support URGENT: verify your account";
    assert_eq!(slot.chars().count(), 40);
    let html = m.body_html.as_deref().expect("html");
    for part in [m.subject.as_str(), m.body_text.as_str(), html] {
        assert!(part.contains(slot), "{part}");
        assert!(!part.contains("evil.example"), "past the slot: {part}");
        assert!(!part.contains("Workspace Row Name"), "{part}");
        assert!(!part.contains("TENANT NOTE"), "{part}");
        assert!(!part.contains("logo.png"), "{part}");
    }
    assert!(!html.contains("<img"), "no logo");
}

/// A workspace may send confirmations to only so many addresses it has never
/// mailed in a day; an address it has mailed before isn't held by that, and
/// a paying workspace gets a higher ceiling.
#[actix_web::test]
async fn a_daily_ceiling_holds_confirmations_to_addresses_never_mailed() {
    use backend::handlers::users::{
        NEW_ADDRESS_CONFIRMATIONS_PER_DAY, NEW_ADDRESS_CONFIRMATIONS_PER_DAY_PAID,
    };
    let c = cloud();
    let (a, b) = (c.seeded.a.workspace_id, c.seeded.b.workspace_id);
    c.first_contacts(a, NEW_ADDRESS_CONFIRMATIONS_PER_DAY as usize);

    assert!(matches!(
        c.confirm(a, "new@victim.example"),
        SendInvitationResult::Capped
    ));
    assert!(c.confirmations("new@victim.example").is_empty());
    // One A has mailed before: a confirmation to it is no new contact.
    assert!(sent(&c.confirm(a, "stranger0@elsewhere.example")));
    // B's ceiling is its own.
    assert!(sent(&c.confirm(b, "new@victim.example")));

    // Paying lifts the ceiling.
    const { assert!(NEW_ADDRESS_CONFIRMATIONS_PER_DAY_PAID > NEW_ADDRESS_CONFIRMATIONS_PER_DAY) };
    diesel::update(backend::schema::workspaces::table.find(a))
        .set(backend::schema::workspaces::billing_status.eq(Some("active")))
        .execute(&mut c.db.conn())
        .expect("paid");
    assert!(sent(&c.confirm(a, "new@victim.example")));
}
