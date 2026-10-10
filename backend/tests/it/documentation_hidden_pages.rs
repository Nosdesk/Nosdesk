//! A documentation page or collection the caller can't see is absent on every
//! docs route: writes answer 404 (400 where the id names a parent or a target
//! collection, like a missing one) and change nothing, and lists leave it out.
//!
//! One page and one collection are restricted to an "insider" agent by a direct
//! user grant. An "outsider" agent and a plain member are refused; the insider
//! and a workspace admin are not.

use std::collections::HashMap;
use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use tokio::sync::RwLock as TokioRwLock;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{
    Claims, DocumentationStatus, NewDocumentationCollection, NewDocumentationCollectionPage,
    NewDocumentationPage, NewTicket,
};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::repository::{
    documentation_collections, documentation_page_tickets, documentation_starred_pages,
    documentation_subscriptions,
};
use backend::services::notifications::NotificationService;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool};

const REF: &str = "test:documentation_hidden_pages";

struct Fixture {
    workspace: WorkspaceContext,
    admin: Uuid,
    member: Uuid,
    insider: Uuid,
    outsider: Uuid,
    /// Restricted to the insider by a page-level grant; in no collection.
    hidden: i32,
    /// No restrictions.
    open: i32,
    /// Restricted to the insider by a collection-level grant.
    secret: i32,
    secret_slug: String,
    /// In `secret`, no page-level rules of its own.
    in_secret: i32,
    /// No restrictions.
    open_collection: i32,
    ticket: i32,
}

fn page(conn: &mut backend::db::DbConnection, title: &str, author: Uuid) -> i32 {
    use backend::schema::documentation_pages;
    diesel::insert_into(documentation_pages::table)
        .values(&NewDocumentationPage {
            uuid: Uuid::new_v4(),
            title: title.to_string(),
            slug: format!(
                "{}-{}",
                title.to_lowercase(),
                &Uuid::new_v4().simple().to_string()[..6]
            ),
            icon: None,
            cover_image: None,
            status: DocumentationStatus::Published,
            created_by: author,
            last_edited_by: author,
            parent_id: None,
            display_order: None,
            is_public: false,
            is_template: false,
            yjs_state_vector: None,
            yjs_document: None,
            yjs_client_id: None,
            has_unsaved_changes: false,
        })
        .returning(documentation_pages::id)
        .get_result(conn)
        .expect("insert page")
}

fn collection(conn: &mut backend::db::DbConnection, name: &str, slug: &str) -> i32 {
    documentation_collections::create_collection(
        conn,
        NewDocumentationCollection {
            uuid: Uuid::now_v7(),
            name: name.to_string(),
            slug: slug.to_string(),
            description: None,
            icon: None,
            color: None,
            is_system: false,
            created_by: None,
        },
    )
    .expect("create collection")
    .id
}

fn setup(pool: &TestPool) -> Fixture {
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let (insider, outsider) = {
        let mut conn = pool.get().expect("conn");
        (
            common::insert_plain_user(&mut conn, "Insider Agent"),
            common::insert_plain_user(&mut conn, "Outsider Agent"),
        )
    };
    let secret_slug = format!("secret-{}", &Uuid::new_v4().simple().to_string()[..8]);

    let (hidden, open, secret, in_secret, open_collection, ticket) =
        run_in_workspace(pool, REF, ws, |c| {
            for agent in [insider, outsider] {
                add_membership(c, ws, agent, "agent", SeatWriteAuthority::ControlPlane)?;
            }
            let author = seeded.a.admin_uuid;
            let hidden = page(c, "Hidden", author);
            backend::repository::set_page_visibility(c, hidden, vec![], vec![insider], None)?;
            let open = page(c, "Open", author);

            let secret = collection(c, "Secret", &secret_slug);
            documentation_collections::set_collection_visibility(
                c,
                secret,
                vec![],
                vec![insider],
                None,
            )?;
            let in_secret = page(c, "InSecret", author);
            documentation_collections::add_page_to_collection(
                c,
                NewDocumentationCollectionPage {
                    collection_id: secret,
                    page_id: in_secret,
                    created_by: None,
                },
            )?;
            let open_collection = collection(
                c,
                "Open",
                &format!("open-{}", &Uuid::new_v4().simple().to_string()[..8]),
            );

            let state = backend::repository::workflow_states::default_state(c)?.id;
            let ticket: i32 = diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: "VPN drops every hour".to_string(),
                    workflow_state_id: state,
                    ..Default::default()
                })
                .returning(backend::schema::tickets::id)
                .get_result(c)?;
            documentation_page_tickets::upsert_link(c, hidden, ticket, "resolves", None)?;

            Ok((hidden, open, secret, in_secret, open_collection, ticket))
        })
        .expect("seed docs");

    Fixture {
        workspace: WorkspaceContext {
            workspace_id: ws,
            workspace_uuid: seeded.a.workspace_uuid,
            slug: seeded.a.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        },
        admin: seeded.a.admin_uuid,
        member: seeded.a.member_uuid,
        insider,
        outsider,
        hidden,
        open,
        secret,
        secret_slug,
        in_secret,
        open_collection,
        ticket,
    }
}

fn claims(user: Uuid) -> Claims {
    Claims {
        sub: user.to_string(),
        name: "Docs Caller".to_string(),
        email: "caller@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

/// The caller rides in a test-only header so one app serves every caller.
const AS: &str = "x-test-as";

fn request(method: Method, uri: &str, user: Uuid, body: Option<Value>) -> http_test::TestRequest {
    let req = http_test::TestRequest::default()
        .method(method)
        .uri(uri)
        .insert_header((AS, user.to_string()));
    match body {
        Some(body) => req.set_json(body),
        None => req,
    }
}

#[actix_web::test]
async fn hidden_pages_and_collections_are_absent_on_every_docs_route() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(6);
    let f = setup(&pool);

    let search_dir = tempfile::tempdir().expect("tempdir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    let notifications =
        NotificationService::new(pool.clone(), Arc::new(TokioRwLock::new(HashMap::new())));
    let workspace = f.workspace.clone();
    let corr = Uuid::now_v7();
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search))
            .app_data(web::Data::new(notifications))
            .wrap_fn(move |req, srv| {
                let user = req
                    .headers()
                    .get(AS)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| Uuid::parse_str(v).ok())
                    .expect("caller header");
                let actor =
                    ActorContext::user(user, Some(corr)).with_workspace(workspace.workspace_id);
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims(user));
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor));
                srv.call(req)
            })
            .service(
                web::scope("/api")
                    .configure(backend::handlers::documentation::config)
                    .configure(backend::handlers::documentation_collections::config),
            ),
    )
    .await;

    // A star the outsider made before the page was restricted.
    run_in_workspace(&pool, REF, f.workspace.workspace_id, |c| {
        documentation_starred_pages::star_page(c, f.outsider, f.hidden)
    })
    .expect("seed star");

    let (hidden, open, secret, in_secret) = (f.hidden, f.open, f.secret, f.in_secret);
    let refused: Vec<(Method, String, Option<Value>, StatusCode)> = vec![
        (
            Method::PUT,
            format!("/api/documentation/pages/{hidden}"),
            Some(json!({ "title": "Gone" })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::DELETE,
            format!("/api/documentation/pages/{hidden}"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            format!("/api/documentation/pages/{hidden}/restore"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::PUT,
            format!("/api/documentation/pages/{hidden}/embeddings"),
            Some(json!({ "embedded_uuids": [] })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            format!("/api/documentation/pages/{hidden}/subscribe"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            format!("/api/documentation/pages/{hidden}/star"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            format!("/api/documentation/pages/{hidden}/verification"),
            Some(json!({})),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::DELETE,
            format!("/api/documentation/pages/{hidden}/verification"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            format!("/api/documentation/pages/{hidden}/tickets"),
            Some(json!({ "ticket_id": f.ticket })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::DELETE,
            format!("/api/documentation/pages/{hidden}/tickets/{}", f.ticket),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::GET,
            format!("/api/documentation/pages/{hidden}/visibility"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::GET,
            format!("/api/documentation/pages/{hidden}/collections"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::PUT,
            format!("/api/documentation/pages/{hidden}/collections"),
            Some(json!({ "collection_ids": [f.open_collection] })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            "/api/documentation/pages/move".to_string(),
            Some(json!({ "page_id": hidden, "new_parent_id": null })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            "/api/documentation/pages/move".to_string(),
            Some(json!({ "page_id": open, "new_parent_id": hidden })),
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::POST,
            "/api/documentation/pages/reorder".to_string(),
            Some(
                json!({ "parent_id": open, "page_orders": [{ "page_id": hidden, "display_order": 0 }] }),
            ),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            "/api/documentation/pages/reorder".to_string(),
            Some(json!({ "parent_id": hidden, "page_orders": [] })),
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::POST,
            "/api/documentation/pages".to_string(),
            Some(json!({ "title": "Child", "parent_id": hidden })),
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::POST,
            "/api/documentation/pages".to_string(),
            Some(json!({ "title": "Inside", "collection_id": secret })),
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::PUT,
            format!("/api/documentation/pages/{open}"),
            Some(json!({ "parent_id": hidden })),
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::PUT,
            format!("/api/documentation/pages/{open}/collections"),
            Some(json!({ "collection_ids": [secret] })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            format!("/api/tickets/{}/documentation/create", f.ticket),
            Some(json!({ "title": "Child", "parent_id": hidden })),
            StatusCode::BAD_REQUEST,
        ),
        (
            Method::GET,
            format!("/api/documentation/collections/{secret}"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::GET,
            format!("/api/documentation/collections/slug/{}", f.secret_slug),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::PUT,
            format!("/api/documentation/collections/{secret}"),
            Some(json!({ "name": "Renamed" })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::GET,
            format!("/api/documentation/collections/{secret}/visibility"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::GET,
            format!("/api/documentation/collections/{secret}/page-overrides"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            format!("/api/documentation/collections/{secret}/pages"),
            Some(json!({ "page_id": open })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            format!("/api/documentation/collections/{}/pages", f.open_collection),
            Some(json!({ "page_id": hidden })),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::DELETE,
            format!("/api/documentation/collections/{secret}/pages/{in_secret}"),
            None,
            StatusCode::NOT_FOUND,
        ),
        (
            Method::POST,
            "/api/documentation/collections/reorder".to_string(),
            Some(json!({ "collection_orders": [{ "collection_id": secret, "display_order": 0 }] })),
            StatusCode::NOT_FOUND,
        ),
    ];
    let mut reached = Vec::new();
    for (method, uri, body, expected) in refused {
        let resp = http_test::call_service(
            &app,
            request(method.clone(), &uri, f.outsider, body).to_request(),
        )
        .await;
        if resp.status() != expected {
            reached.push(format!(
                "{method} {uri}: {} (want {expected})",
                resp.status()
            ));
        }
    }
    assert!(
        reached.is_empty(),
        "the outsider got through:\n{}",
        reached.join("\n")
    );

    // Nothing the outsider tried landed.
    let (
        title,
        status,
        verified,
        subscriptions,
        links,
        embeds,
        memberships,
        secret_name,
        open_parent,
    ) = run_in_workspace(&pool, REF, f.workspace.workspace_id, |c| {
        use backend::schema::{
            documentation_collection_pages as dcp, documentation_collections as dc,
            documentation_page_embeddings as dpe, documentation_page_tickets as dpt,
            documentation_pages as dp, documentation_subscriptions as ds,
        };
        let (title, status, verified): (
            String,
            DocumentationStatus,
            Option<chrono::NaiveDateTime>,
        ) = dp::table
            .find(hidden)
            .select((dp::title, dp::status, dp::verified_at))
            .first(c)?;
        let subscriptions: i64 = ds::table
            .filter(ds::page_id.eq(hidden))
            .count()
            .get_result(c)?;
        let links: i64 = dpt::table
            .filter(dpt::page_id.eq(hidden))
            .count()
            .get_result(c)?;
        let embeds: i64 = dpe::table
            .filter(dpe::source_page_id.eq(hidden))
            .count()
            .get_result(c)?;
        let memberships: Vec<(i32, i32)> = dcp::table
            .filter(dcp::page_id.eq_any([hidden, open, in_secret]))
            .select((dcp::page_id, dcp::collection_id))
            .load(c)?;
        let secret_name: String = dc::table.find(secret).select(dc::name).first(c)?;
        let open_parent: Option<i32> = dp::table.find(open).select(dp::parent_id).first(c)?;
        Ok((
            title,
            status,
            verified,
            subscriptions,
            links,
            embeds,
            memberships,
            secret_name,
            open_parent,
        ))
    })
    .expect("read back");
    assert_eq!(title, "Hidden");
    assert_eq!(status, DocumentationStatus::Published);
    assert_eq!(verified, None);
    assert_eq!(subscriptions, 0);
    assert_eq!(links, 1, "only the seeded link");
    assert_eq!(embeds, 0);
    assert_eq!(memberships, vec![(in_secret, secret)]);
    assert_eq!(secret_name, "Secret");
    assert_eq!(open_parent, None);

    // Lists leave the hidden page and the secret collection's page out.
    let get_json = |uri: String, user: Uuid| {
        let app = &app;
        async move {
            let resp =
                http_test::call_service(app, request(Method::GET, &uri, user, None).to_request())
                    .await;
            assert_eq!(resp.status(), StatusCode::OK, "GET {uri}");
            let body: Value = http_test::read_body_json(resp).await;
            body
        }
    };
    let ids = |rows: &Value, key: &str| -> Vec<i64> {
        rows.as_array()
            .expect("array")
            .iter()
            .filter_map(|r| r[key].as_i64())
            .collect()
    };
    for user in [f.outsider, f.member] {
        let uncollected = get_json("/api/documentation/pages/uncollected".into(), user).await;
        assert!(!ids(&uncollected, "id").contains(&(hidden as i64)));
        assert!(ids(&uncollected, "id").contains(&(open as i64)));
        let secret_resp = http_test::call_service(
            &app,
            request(
                Method::GET,
                &format!("/api/documentation/collections/{secret}"),
                user,
                None,
            )
            .to_request(),
        )
        .await;
        assert_eq!(secret_resp.status(), StatusCode::NOT_FOUND);
    }
    let export = get_json("/api/documentation/pages/export".into(), f.outsider).await;
    assert!(!ids(&export, "id").contains(&(hidden as i64)));
    assert!(!ids(&export, "id").contains(&(in_secret as i64)));
    assert!(ids(&export, "id").contains(&(open as i64)));
    let starred = get_json("/api/documentation/starred".into(), f.outsider).await;
    assert!(
        ids(&starred, "page_id").is_empty(),
        "a star on a page they can't open: {starred}"
    );

    // Creating a doc from the ticket gives the outsider a page of their own
    // rather than the hidden one already linked to it.
    let created = http_test::call_service(
        &app,
        request(
            Method::POST,
            &format!("/api/tickets/{}/documentation/create", f.ticket),
            f.outsider,
            Some(json!({ "title": "My notes" })),
        )
        .to_request(),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created: Value = http_test::read_body_json(created).await;
    assert_ne!(created["id"].as_i64(), Some(hidden as i64));

    // The insider and an admin can do what the outsider couldn't.
    for (user, title) in [(f.insider, "Insider edit"), (f.admin, "Admin edit")] {
        let resp = http_test::call_service(
            &app,
            request(
                Method::PUT,
                &format!("/api/documentation/pages/{hidden}"),
                user,
                Some(json!({ "title": title })),
            )
            .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "{title}");
        let collection = get_json(format!("/api/documentation/collections/{secret}"), user).await;
        assert!(ids(&collection["pages"], "id").contains(&(in_secret as i64)));
    }
    let star = http_test::call_service(
        &app,
        request(
            Method::POST,
            &format!("/api/documentation/pages/{hidden}/star"),
            f.insider,
            None,
        )
        .to_request(),
    )
    .await;
    assert_eq!(star.status(), StatusCode::OK);
}

#[test]
fn an_update_reaches_only_subscribers_who_can_open_the_page() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let f = setup(&pool);

    let readers = run_in_workspace(&pool, REF, f.workspace.workspace_id, |c| {
        for user in [f.admin, f.member, f.insider, f.outsider] {
            documentation_subscriptions::subscribe_user(c, user, f.hidden)?;
        }
        documentation_subscriptions::get_page_subscribers_who_can_read(c, f.hidden)
    })
    .expect("subscribers");

    let mut readers = readers;
    readers.sort();
    let mut expected = vec![f.admin, f.insider];
    expected.sort();
    assert_eq!(readers, expected);
}

/// The docs and sync routes, with the caller riding in [`AS`].
macro_rules! docs_and_sync_app {
    ($pool:expr, $workspace:expr) => {{
        let search_dir = tempfile::tempdir().expect("tempdir");
        let search = Arc::new(SearchService::new(search_dir.path(), &$pool).expect("init search"));
        std::mem::forget(search_dir);
        let notifications =
            NotificationService::new($pool.clone(), Arc::new(TokioRwLock::new(HashMap::new())));
        let workspace: WorkspaceContext = $workspace;
        let corr = Uuid::now_v7();
        http_test::init_service(
            App::new()
                .app_data(web::Data::new($pool.clone()))
                .app_data(web::Data::new(search))
                .app_data(web::Data::new(notifications))
                .wrap_fn(move |req, srv| {
                    let user = req
                        .headers()
                        .get(AS)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| Uuid::parse_str(v).ok())
                        .expect("caller header");
                    let actor =
                        ActorContext::user(user, Some(corr)).with_workspace(workspace.workspace_id);
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims(user));
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor));
                    srv.call(req)
                })
                .service(
                    web::scope("/api")
                        .configure(backend::handlers::documentation::config)
                        .configure(backend::handlers::documentation_collections::config)
                        .configure(backend::handlers::sync::config),
                ),
        )
        .await
    }};
}

/// A position in the feed: `(from, from_xid8)` for the next delta.
type Cursor = (i64, i64);

/// The newest action recorded in the workspace.
fn newest_action(pool: &TestPool, ws: i32) -> i64 {
    use backend::schema::sync_actions;
    run_in_workspace(pool, REF, ws, |c| {
        sync_actions::table
            .select(diesel::dsl::max(sync_actions::sync_id))
            .first::<Option<i64>>(c)
    })
    .expect("newest action")
    .unwrap_or(0)
}

/// `user`'s delta after `cursor`, once it covers everything recorded so far,
/// and the cursor after it. The delta serves only settled rows (below the
/// cluster-wide commit horizon), so another test's open transaction can hold
/// the newest rows back for a moment.
macro_rules! delta_after {
    ($app:expr, $f:expr, $pool:expr, $user:expr, $cursor:expr) => {{
        let newest = newest_action(&$pool, $f.workspace.workspace_id);
        let (from, from_xid8): Cursor = $cursor;
        let uri = format!(
            "/api/sync/delta?from={from}&from_xid8={from_xid8}&limit=5000&groups=workspace:{}",
            $f.workspace.workspace_id
        );
        let mut out: Option<(Vec<Value>, Cursor)> = None;
        for _ in 0..200 {
            let resp = http_test::call_service(
                &$app,
                request(Method::GET, &uri, $user, None).to_request(),
            )
            .await;
            assert_eq!(resp.status(), StatusCode::OK, "GET {uri}");
            let body: Value = http_test::read_body_json(resp).await;
            let last = body["last_sync_id"].as_i64().expect("last_sync_id");
            if last >= newest {
                out = Some((
                    body["actions"].as_array().expect("actions").clone(),
                    (last, body["last_xid8"].as_i64().expect("last_xid8")),
                ));
                break;
            }
            actix_web::rt::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        out.unwrap_or_else(|| panic!("the delta never reached the newest action ({newest})"))
    }};
}

/// The rows in `rows` about one documentation record.
fn about<'a>(rows: &'a [Value], aggregate: &str, id: i32) -> Vec<&'a Value> {
    rows.iter()
        .filter(|r| {
            r["aggregate"] == json!(aggregate) && r["aggregate_id"] == json!(id.to_string())
        })
        .collect()
}

/// Every row about the record is a delete naming only its id, with nothing
/// about who changed it or when.
fn only_deletes(rows: &[Value], aggregate: &str, id: i32) -> bool {
    let rows = about(rows, aggregate, id);
    let bare = |r: &Value| {
        r.as_object().is_some_and(|o| {
            o.keys().all(|k| {
                matches!(
                    k.as_str(),
                    "sync_id"
                        | "aggregate"
                        | "aggregate_id"
                        | "op"
                        | "event_type"
                        | "schema_version"
                        | "data"
                        | "groups"
                )
            })
        })
    };
    !rows.is_empty()
        && rows
            .iter()
            .all(|r| r["op"] == json!("D") && r["data"] == json!({ "id": id }) && bare(r))
}

/// A row about the record that carries it whole (a `title` or a `name`).
fn carries_row(rows: &[Value], aggregate: &str, id: i32) -> bool {
    about(rows, aggregate, id).iter().any(|r| {
        r["op"] != json!("D")
            && (r["data"].get("title").is_some() || r["data"].get("name").is_some())
    })
}

/// Losing access to a page or a collection reaches the client as a delete of
/// each record it can no longer open; gaining access sends the record.
#[actix_web::test]
async fn losing_access_reads_as_a_delete_and_gaining_it_sends_the_row() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(6);
    let f = setup(&pool);
    let ws = f.workspace.workspace_id;
    let in_open = run_in_workspace(&pool, REF, ws, |c| {
        let page_id = page(c, "InOpen", f.admin);
        documentation_collections::add_page_to_collection(
            c,
            NewDocumentationCollectionPage {
                collection_id: f.open_collection,
                page_id,
                created_by: None,
            },
        )?;
        Ok(page_id)
    })
    .expect("seed page in the open collection");
    let app = docs_and_sync_app!(pool, f.workspace.clone());

    // A collection restricted away from the outsider.
    let (_, before) = delta_after!(app, f, pool, f.outsider, (0, 0));
    run_in_workspace(&pool, REF, ws, |c| {
        documentation_collections::set_collection_visibility(
            c,
            f.open_collection,
            vec![],
            vec![f.insider],
            None,
        )
    })
    .expect("restrict the collection");
    let (rows, after_collection) = delta_after!(app, f, pool, f.outsider, before);
    assert!(
        only_deletes(&rows, "documentation_collection", f.open_collection),
        "the outsider's delta deletes the collection: {rows:?}"
    );
    assert!(
        only_deletes(&rows, "documentation_page", in_open),
        "the outsider's delta deletes the collection's page: {rows:?}"
    );
    let (insider_rows, _) = delta_after!(app, f, pool, f.insider, before);
    assert!(
        carries_row(&insider_rows, "documentation_collection", f.open_collection),
        "the insider keeps the collection: {insider_rows:?}"
    );

    // A page restricted away from the outsider.
    run_in_workspace(&pool, REF, ws, |c| {
        backend::repository::set_page_visibility(c, f.open, vec![], vec![f.insider], None)
    })
    .expect("restrict the page");
    let (rows, after_page) = delta_after!(app, f, pool, f.outsider, after_collection);
    assert!(
        only_deletes(&rows, "documentation_page", f.open),
        "the outsider's delta deletes the page: {rows:?}"
    );

    // A page opened to the outsider.
    run_in_workspace(&pool, REF, ws, |c| {
        backend::repository::set_page_visibility(
            c,
            f.hidden,
            vec![],
            vec![f.insider, f.outsider],
            None,
        )
    })
    .expect("share the page");
    let (rows, _) = delta_after!(app, f, pool, f.outsider, after_page);
    assert!(
        carries_row(&rows, "documentation_page", f.hidden),
        "the outsider's delta sends the page they can now open: {rows:?}"
    );
    let row = about(&rows, "documentation_page", f.hidden)
        .into_iter()
        .rfind(|r| r["op"] != json!("D"))
        .expect("a page row");
    assert_eq!(row["data"]["title"], json!("Hidden"));
}

/// A collection created for some people is restricted as it is created, so
/// the live stream never sends it to anyone else.
#[actix_web::test]
async fn a_collection_created_for_some_people_never_reaches_the_rest() {
    use backend::handlers::sse::{Envelope, SseEvent, SseState, SseStream};
    use backend::handlers::sync::delta::ActionRow;
    use backend::sync::visibility::SyncViewer;
    use futures::StreamExt;

    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(6);
    let f = setup(&pool);
    let ws = f.workspace.workspace_id;
    let app = docs_and_sync_app!(pool, f.workspace.clone());

    let resp = http_test::call_service(
        &app,
        request(
            Method::POST,
            "/api/documentation/collections",
            f.admin,
            Some(json!({
                "name": "Payroll",
                "slug": format!("payroll-{}", &Uuid::new_v4().simple().to_string()[..8]),
                "visible_to_user_uuids": [f.insider],
            })),
        )
        .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: Value = http_test::read_body_json(resp).await;
    let id = created["id"].as_i64().expect("collection id") as i32;

    let refused = http_test::call_service(
        &app,
        request(
            Method::GET,
            &format!("/api/documentation/collections/{id}"),
            f.outsider,
            None,
        )
        .to_request(),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::NOT_FOUND);

    // Every action about the collection, as the live stream carries them.
    let actions: Vec<Value> = run_in_workspace(&pool, REF, ws, |c| {
        use backend::schema::sync_actions;
        sync_actions::table
            .filter(sync_actions::aggregate_id.eq(id.to_string()))
            .filter(
                sync_actions::aggregate.eq(backend::models::SyncAggregate::DocumentationCollection),
            )
            .order(sync_actions::sync_id.asc())
            .select((
                sync_actions::sync_id,
                sync_actions::aggregate,
                sync_actions::aggregate_id,
                sync_actions::op,
                sync_actions::event_type,
                sync_actions::schema_version,
                sync_actions::data,
                sync_actions::groups,
                sync_actions::actor_uuid,
                sync_actions::actor_kind,
                sync_actions::actor_ref,
                sync_actions::correlation_id,
                sync_actions::causation_id,
                sync_actions::occurred_at,
                sync_actions::xid8,
            ))
            .load::<ActionRow>(c)
    })
    .expect("load actions")
    .iter()
    .map(|r| serde_json::to_value(r).expect("row json"))
    .collect();
    assert!(
        actions
            .iter()
            .any(|a| a["event_type"] == json!("documentation_collection.created")),
        "the create is recorded: {actions:?}"
    );

    let (viewer, allowed) = run_in_workspace(&pool, REF, ws, |c| {
        let user = backend::repository::users::get_user_by_uuid(&f.outsider, c)?;
        Ok::<_, diesel::result::Error>((
            SyncViewer::resolve(c, &user),
            backend::sync::groups::allowed_for_user(c, &user)?,
        ))
    })
    .expect("viewer");
    let registry =
        Arc::new(backend::services::connection_registry::ConnectionRegistry::with_limits(4, 16));
    let guard = registry
        .try_acquire((f.outsider, ws))
        .expect("connection slot");
    let mut stream = SseStream::new(
        Vec::new(),
        vec![Envelope {
            id: 1,
            event: SseEvent::SyncActions {
                actions: Value::Array(actions),
                last_xid8: 0,
                last_sync_id: 0,
                timestamp: chrono::Utc::now(),
            },
            source_client_id: None,
        }],
        format!("outsider-{}", f.outsider),
        web::Data::new(SseState::new()),
        web::Data::new(pool.clone()),
        viewer,
        Some(ws),
        Arc::new(
            allowed
                .into_iter()
                .collect::<std::collections::HashSet<String>>(),
        ),
        guard,
    );
    let frame = stream.next().await.expect("a frame").expect("frame bytes");
    let frame = String::from_utf8(frame.to_vec()).expect("utf8 frame");
    let data = frame
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .expect("data line");
    let event: Value = serde_json::from_str(data).expect("frame json");
    let sent = event
        .get("actions")
        .or_else(|| event.get("data").and_then(|d| d.get("actions")))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| panic!("no actions in frame: {data}"));
    assert!(
        !carries_row(&sent, "documentation_collection", id),
        "the outsider's stream carries the collection: {sent:?}"
    );
}

/// A collection's page count, as the list reports it, counts only the pages
/// the caller can open.
#[actix_web::test]
async fn a_collections_page_count_leaves_out_pages_the_caller_cant_open() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(6);
    let f = setup(&pool);
    run_in_workspace(&pool, REF, f.workspace.workspace_id, |c| {
        for page_id in [f.open, f.hidden] {
            documentation_collections::add_page_to_collection(
                c,
                NewDocumentationCollectionPage {
                    collection_id: f.open_collection,
                    page_id,
                    created_by: None,
                },
            )?;
        }
        Ok(())
    })
    .expect("fill the open collection");
    let app = docs_and_sync_app!(pool, f.workspace.clone());

    for (user, expected) in [(f.outsider, 1), (f.member, 1), (f.insider, 2), (f.admin, 2)] {
        let resp = http_test::call_service(
            &app,
            request(Method::GET, "/api/documentation/collections", user, None).to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let list: Value = http_test::read_body_json(resp).await;
        let row = list
            .as_array()
            .expect("array")
            .iter()
            .find(|c| c["id"] == json!(f.open_collection))
            .unwrap_or_else(|| panic!("the open collection is listed: {list}"));
        assert_eq!(row["page_count"], json!(expected), "{user}: {row}");
    }
}

/// A restricted page or collection that is deleted for good reaches a reader
/// who couldn't open it only as deletes, even from the start of the feed.
#[actix_web::test]
async fn a_deleted_restricted_record_never_replays_its_row() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(6);
    let f = setup(&pool);
    run_in_workspace(&pool, REF, f.workspace.workspace_id, |c| {
        backend::repository::permanently_delete_page(f.hidden, c)?;
        documentation_collections::delete_collection(c, f.secret)?;
        Ok(())
    })
    .expect("delete the restricted page and collection");
    let app = docs_and_sync_app!(pool, f.workspace.clone());

    let (rows, _) = delta_after!(app, f, pool, f.outsider, (0, 0));
    assert!(
        only_deletes(&rows, "documentation_page", f.hidden),
        "the deleted page reaches the outsider only as deletes: {:?}",
        about(&rows, "documentation_page", f.hidden)
    );
    assert!(
        only_deletes(&rows, "documentation_collection", f.secret),
        "the deleted collection reaches the outsider only as deletes: {:?}",
        about(&rows, "documentation_collection", f.secret)
    );
}

/// A copy of a page is closed to the same people as the original: it goes
/// in the original's collection and takes its page-level rules.
#[actix_web::test]
async fn a_copy_is_closed_to_the_same_people() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(6);
    let f = setup(&pool);
    let app = docs_and_sync_app!(pool, f.workspace.clone());

    for (source, what) in [
        (f.hidden, "a page shared by its own rules"),
        (f.in_secret, "a page in a restricted collection"),
    ] {
        let resp = http_test::call_service(
            &app,
            request(
                Method::POST,
                "/api/documentation/pages",
                f.insider,
                Some(json!({ "title": "Copy", "copy_access_from": source })),
            )
            .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED, "{what}");
        let copy: Value = http_test::read_body_json(resp).await;
        let copy = copy["id"].as_i64().expect("copy id");
        for (user, expected) in [
            (f.outsider, StatusCode::NOT_FOUND),
            (f.insider, StatusCode::OK),
        ] {
            let resp = http_test::call_service(
                &app,
                request(
                    Method::GET,
                    &format!("/api/documentation/pages/{copy}"),
                    user,
                    None,
                )
                .to_request(),
            )
            .await;
            assert_eq!(resp.status(), expected, "{what}: {user}");
        }
    }

    // A page the caller can't open can't be copied.
    let resp = http_test::call_service(
        &app,
        request(
            Method::POST,
            "/api/documentation/pages",
            f.outsider,
            Some(json!({ "title": "Copy", "copy_access_from": f.hidden })),
        )
        .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
