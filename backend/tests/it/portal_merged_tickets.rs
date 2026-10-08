//! A merged request in the portal. Its conversation moved to the request it
//! was merged into, so the portal says where it went (when the requester can
//! see that request) and a reply sent to it lands there; when the requester
//! can't see the destination, the reply is refused rather than left on the
//! merged request where nobody reads it.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::handlers::portal::PortalContext;
use backend::middleware::RequestContext;
use backend::models::{NewTicket, Ticket};
use backend::repository::ticket_merge::{execute_merge, MergeInput};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:portal_merged_tickets";

struct Fixture {
    _db: common::TestDb,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
    pool: TestPool,
    storage: Arc<dyn Storage>,
    search: Arc<SearchService>,
    ws: WorkspaceSeed,
    /// Another requester in the same workspace.
    other: Uuid,
}

impl Fixture {
    fn new() -> Self {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
        let other = common::insert_plain_user(&mut pool.get().expect("conn"), "Other Requester");
        let a = ws.workspace_id;
        run_in_workspace(&pool, REF, a, |c| {
            add_membership(c, a, other, "member", SeatWriteAuthority::ControlPlane)
        })
        .expect("member");
        let storage_dir = tempfile::tempdir().expect("storage dir");
        let storage = create_storage(StorageConfig::Local {
            base_path: storage_dir.path().to_string_lossy().into_owned(),
        });
        let search_dir = tempfile::tempdir().expect("search dir");
        let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
        Self {
            _db: db,
            _dirs: (storage_dir, search_dir),
            pool,
            storage,
            search,
            ws,
            other,
        }
    }

    fn ticket(&self, title: &str, requester: Uuid) -> Ticket {
        run_in_workspace(&self.pool, REF, self.ws.workspace_id, |c| {
            let state = backend::repository::workflow_states::default_state(c)?;
            diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: state.id,
                    requester_uuid: Some(requester),
                    ..Default::default()
                })
                .get_result(c)
        })
        .expect("insert ticket")
    }

    /// An agent merges `source` into `destination`.
    fn merge(&self, source: &Ticket, destination: &Ticket) {
        let actor =
            ActorContext::user(self.ws.admin_uuid, None).with_workspace(self.ws.workspace_id);
        execute_merge(
            &mut self.pool.get().expect("conn"),
            MergeInput {
                destination_ticket_id: destination.id,
                source_ticket_ids: vec![source.id],
                reason: None,
                notify_customer: false,
                expected_state: Vec::new(),
                marker_body: None,
            },
            &actor,
        )
        .expect("merge");
    }

    /// Public replies on `ticket_id` with `content`.
    fn replies(&self, ticket_id: i32, content: &str) -> i64 {
        use backend::schema::comments;
        run_in_workspace(&self.pool, REF, self.ws.workspace_id, |c| {
            comments::table
                .filter(comments::ticket_id.eq(ticket_id))
                .filter(comments::content.eq(content))
                .count()
                .get_result(c)
        })
        .expect("count replies")
    }
}

/// The portal routes the requester `user` uses, as its middlewares would set
/// them up.
macro_rules! portal_as {
    ($fx:expr, $user:expr) => {{
        let user: Uuid = $user;
        let a = $fx.ws.workspace_id;
        let workspace = WorkspaceContext {
            workspace_id: a,
            workspace_uuid: $fx.ws.workspace_uuid,
            slug: $fx.ws.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let portal = PortalContext {
            user_uuid: user,
            workspace_id: a,
            workspace_uuid: $fx.ws.workspace_uuid,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user, Some(corr)).with_workspace(a);
        http_test::init_service(
            App::new()
                .app_data(web::Data::new($fx.pool.clone()))
                .app_data(web::Data::new($fx.storage.clone()))
                .app_data(web::Data::new($fx.search.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(portal.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .route(
                    "/api/portal/tickets/{id}",
                    web::get().to(backend::handlers::portal::get_my_ticket),
                )
                .route(
                    "/api/portal/tickets/{id}/comments",
                    web::post().to(backend::handlers::portal::reply_to_my_ticket),
                ),
        )
        .await
    }};
}

macro_rules! reply {
    ($app:expr, $ticket:expr, $content:expr) => {
        http_test::call_service(
            $app,
            http_test::TestRequest::post()
                .uri(&format!("/api/portal/tickets/{}/comments", $ticket))
                .set_json(json!({ "content": $content }))
                .to_request(),
        )
        .await
    };
}

macro_rules! detail {
    ($app:expr, $ticket:expr) => {{
        let resp = http_test::call_service(
            $app,
            http_test::TestRequest::get()
                .uri(&format!("/api/portal/tickets/{}", $ticket))
                .to_request(),
        )
        .await;
        assert!(resp.status().is_success(), "detail: {}", resp.status());
        let body: serde_json::Value = http_test::read_body_json(resp).await;
        body
    }};
}

/// Both requests are the requester's: the merged one points at the other and
/// a reply sent to it lands there.
#[actix_web::test]
async fn a_reply_to_a_merged_request_lands_on_the_one_it_was_merged_into() {
    let fx = Fixture::new();
    let requester = fx.ws.member_uuid;
    let destination = fx.ticket("Laptop won't boot", requester);
    let source = fx.ticket("Laptop still won't boot", requester);
    fx.merge(&source, &destination);
    let app = portal_as!(fx, requester);

    let shown = detail!(&app, source.id);
    assert_eq!(
        shown["merged_into"],
        json!(destination.number),
        "the merged request says where it went: {shown}"
    );

    let resp = reply!(&app, source.id, "It's still doing it");
    assert!(resp.status().is_success(), "reply: {}", resp.status());
    assert_eq!(fx.replies(destination.id, "It's still doing it"), 1);
    assert_eq!(fx.replies(source.id, "It's still doing it"), 0);
}

/// The destination is someone else's request: the merged one doesn't name it,
/// and a reply is refused instead of landing where nobody reads it.
#[actix_web::test]
async fn a_reply_to_a_request_merged_out_of_sight_is_refused() {
    let fx = Fixture::new();
    let requester = fx.ws.member_uuid;
    let destination = fx.ticket("Printer offline", fx.other);
    let source = fx.ticket("Printer on level 3 offline", requester);
    fx.merge(&source, &destination);
    let app = portal_as!(fx, requester);

    let shown = detail!(&app, source.id);
    assert_eq!(shown["merged_into"], json!(null), "{shown}");

    let resp = reply!(&app, source.id, "Any news?");
    assert_eq!(resp.status().as_u16(), 409, "refused as merged");
    assert_eq!(fx.replies(destination.id, "Any news?"), 0);
    assert_eq!(fx.replies(source.id, "Any news?"), 0);
}
