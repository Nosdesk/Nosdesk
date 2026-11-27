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
