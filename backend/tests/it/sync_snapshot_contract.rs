//! What each group's bootstrap snapshot carries.
//!
//! The client treats a snapshot for a group as the whole of that group: a row
//! the snapshot leaves out is removed from the pool. A loader that stops
//! sending a row is then no longer a missing refresh but data loss on every
//! device, so these pin the set each group's snapshot carries, through the
//! real handler, for a staff viewer:
//!
//! - `ticket:<id>`: the ticket, every comment on it (internal ones too), their
//!   attachments, its device links, its ticket links, its project links and
//!   the projects they name.
//! - `project:<id>`: the project, every link to it and every linked ticket.
//! - the workspace group: every ticket, project, project link, collection and
//!   page (any status) in the workspace.
//! - every group: every workflow state (archived ones too), every asset, and
//!   the workspace's people as `repository::directory::people` defines them
//!   plus the viewer, and no one else.
//!
//! A bootstrap for several groups carries the union of what each carries
//! alone.

use std::collections::{BTreeMap, BTreeSet};

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use diesel::sql_types::Integer;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewComment, NewTicket};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_context};

use crate::common::{self, TestPool, TwoWorkspaces, WorkspaceSeed};

const REF: &str = "test:sync_snapshot_contract";

/// Workspace A's records, as the snapshots should carry them. Every
/// `BTreeSet` is keyed the way [`Snapshot`] keys rows.
struct Fixture {
    pool: TestPool,
    ws: TwoWorkspaces,
    /// A member of B only, who requested `t3` in A.
    requester_from_b: Uuid,
    /// In project `p`, with a public and an internal comment, an attachment
    /// on each, a device link, and a link to `t2` (both directions).
    t1: i32,
    /// In project `p`.
    t2: i32,
    /// In no project, closed, requested by someone from B.
    t3: i32,
    p: i32,
    /// A project with no tickets.
    q: i32,
    comments: BTreeSet<String>,
    attachments: BTreeSet<String>,
    asset: i32,
    collection: i32,
    /// Published (in the collection), draft, archived and deleted.
    pages: BTreeSet<String>,
    /// Every workflow state in A, the archived one included.
    states: BTreeSet<String>,
}

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// Run an `INSERT ... RETURNING id`.
fn insert_id(c: &mut backend::db::DbConnection, sql: &str) -> QueryResult<i32> {
    Ok(diesel::sql_query(sql).get_result::<Id>(c)?.id)
}

fn keys(ids: &[i32]) -> BTreeSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

fn pair(a: i32, b: i32) -> String {
    format!("{a}:{b}")
}

fn seed(db: &common::TestDb) -> Fixture {
    common::ensure_test_keyring();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let ws = common::seed_two_workspaces(&mut conn);
    let (a, b) = (ws.a.workspace_id, ws.b.workspace_id);
    let admin = ws.a.admin_uuid;

    let requester_from_b = common::insert_plain_user(&mut conn, "Requester From B");
    with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &ActorContext::user(ws.b.admin_uuid, None).with_workspace(b),
        |c| {
            add_membership(
                c,
                b,
                requester_from_b,
                "member",
                SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        },
    )
    .expect("join B");
    drop(conn);

    run_in_workspace(&pool, REF, a, |c| {
        let open = backend::repository::workflow_states::default_state(c)?;
        let done = insert_id(
            c,
            "INSERT INTO workflow_states (name, category, color, position, is_default) \
             VALUES ('Shipped', 'done', 'green', 90, false) RETURNING id",
        )?;
        insert_id(
            c,
            "INSERT INTO workflow_states (name, category, color, position, is_default, archived_at) \
             VALUES ('Retired', 'backlog', 'slate', 91, false, now()) RETURNING id",
        )?;
        let states: Vec<i32> = backend::schema::workflow_states::table
            .select(backend::schema::workflow_states::id)
            .load(c)?;

        let ticket = |c: &mut backend::db::DbConnection,
                      title: &str,
                      state: i32,
                      requester: Uuid|
         -> QueryResult<i32> {
            let t: backend::models::Ticket = diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: state,
                    requester_uuid: Some(requester),
                    ..Default::default()
                })
                .get_result(c)?;
            Ok(t.id)
        };
        let t1 = ticket(c, "Printer jams", open.id, ws.a.member_uuid)?;
        let t2 = ticket(c, "Toner low", open.id, ws.a.member_uuid)?;
        let t3 = ticket(c, "Badge reader", done, requester_from_b)?;

        let comment = |c: &mut backend::db::DbConnection, is_internal: bool| -> QueryResult<i32> {
            let row: backend::models::Comment =
                diesel::insert_into(backend::schema::comments::table)
                    .values(&NewComment {
                        content: "Looking into it".to_string(),
                        ticket_id: t1,
                        user_uuid: admin,
                        is_internal,
                        ..Default::default()
                    })
                    .get_result(c)?;
            Ok(row.id)
        };
        let public = comment(c, false)?;
        let internal = comment(c, true)?;
        let mut attachments = Vec::new();
        for comment_id in [public, internal] {
            attachments.push(insert_id(
                c,
                &format!(
                    "INSERT INTO attachments (url, name, comment_id) \
                     VALUES ('/uploads/a{comment_id}.png', 'shot.png', {comment_id}) RETURNING id"
                ),
            )?);
        }
        // A comment on another ticket, which `ticket:t1` must not carry.
        diesel::insert_into(backend::schema::comments::table)
            .values(&NewComment {
                content: "Elsewhere".to_string(),
                ticket_id: t2,
                user_uuid: admin,
                ..Default::default()
            })
            .execute(c)?;

        let asset = insert_id(
            c,
            "INSERT INTO assets (name) VALUES ('Front desk printer') RETURNING id",
        )?;
        diesel::sql_query(format!(
            "INSERT INTO ticket_assets (ticket_id, asset_id) VALUES ({t1}, {asset})"
        ))
        .execute(c)?;
        diesel::sql_query(format!(
            "INSERT INTO linked_tickets (ticket_id, linked_ticket_id, relation_type) \
             VALUES ({t1}, {t2}, 'related'), ({t2}, {t1}, 'related')"
        ))
        .execute(c)?;

        let p = insert_id(
            c,
            "INSERT INTO projects (name) VALUES ('Office move') RETURNING id",
        )?;
        let q = insert_id(
            c,
            "INSERT INTO projects (name) VALUES ('Quiet project') RETURNING id",
        )?;
        diesel::sql_query(format!(
            "INSERT INTO project_tickets (project_id, ticket_id, display_order) \
             VALUES ({p}, {t1}, 0), ({p}, {t2}, 1)"
        ))
        .execute(c)?;

        let collection = insert_id(
            c,
            "INSERT INTO documentation_collections (name, slug) \
             VALUES ('Runbooks', 'runbooks') RETURNING id",
        )?;
        let mut pages = Vec::new();
        for status in ["published", "draft", "archived", "deleted"] {
            pages.push(insert_id(
                c,
                &format!(
                    "INSERT INTO documentation_pages (title, slug, status, created_by, last_edited_by) \
                     VALUES ('Page {status}', 'page-{status}', '{status}', '{admin}', '{admin}') \
                     RETURNING id"
                ),
            )?);
        }
        diesel::sql_query(format!(
            "INSERT INTO documentation_collection_pages (collection_id, page_id) \
             VALUES ({collection}, {})",
            pages[0]
        ))
        .execute(c)?;

        Ok(Fixture {
            pool: pool.clone(),
            ws: ws.clone(),
            requester_from_b,
            t1,
            t2,
            t3,
            p,
            q,
            comments: keys(&[public, internal]),
            attachments: keys(&attachments),
            asset,
            collection,
            pages: keys(&pages),
            states: keys(&states),
        })
    })
    .expect("seed A's records")
}

/// One bootstrap response: the granted groups and every row, as the set of
/// keys per `__model__`. Composite rows key as `left:right`.
struct Snapshot {
    granted: Vec<String>,
    rows: BTreeMap<String, BTreeSet<String>>,
}

impl Snapshot {
    fn of(&self, model: &str) -> BTreeSet<String> {
        self.rows.get(model).cloned().unwrap_or_default()
    }
}

fn row_key(model: &str, row: &serde_json::Value) -> String {
    let field = |name: &str| match &row[name] {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    match model {
        "user" => field("uuid"),
        "project_ticket" => format!("{}:{}", field("project_id"), field("ticket_id")),
        "ticket_asset" => format!("{}:{}", field("ticket_id"), field("asset_id")),
        "linked_ticket" => format!("{}:{}", field("ticket_id"), field("linked_ticket_id")),
        _ => field("id"),
    }
}

/// GET a bootstrap for `groups` as `viewer`, with `seed`'s workspace
/// resolved for the request.
async fn bootstrap(pool: &TestPool, seed: &WorkspaceSeed, viewer: Uuid, groups: &str) -> Snapshot {
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Viewer".to_string(),
        email: format!("{}@contract.test", viewer.simple()),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: seed.workspace_id,
        workspace_uuid: seed.workspace_uuid,
        slug: seed.slug.clone(),
        name: seed.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(viewer, Some(corr)).with_workspace(seed.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::sync::config)),
    )
    .await;
    let uri = format!("/api/sync/bootstrap?groups={groups}&schema=test");
    let resp =
        http_test::call_service(&app, http_test::TestRequest::get().uri(&uri).to_request()).await;
    assert_eq!(resp.status(), StatusCode::OK, "GET {uri}");
    let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8 body");

    let mut granted = None;
    let mut ended = false;
    let mut rows: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line).expect("NDJSON line");
        assert!(value.get("__error__").is_none(), "{groups}: {line}");
        if let Some(meta) = value.get("__meta__") {
            granted = Some(
                serde_json::from_value::<Vec<String>>(meta["groups_granted"].clone())
                    .expect("groups_granted"),
            );
        } else if value.get("__end__").is_some() {
            ended = true;
        } else if let Some(model) = value["__model__"].as_str() {
            rows.entry(model.to_string())
                .or_default()
                .insert(row_key(model, &value));
        }
    }
    assert!(ended, "{groups}: the stream completes");
    Snapshot {
        granted: granted.expect("a __meta__ header"),
        rows,
    }
}

impl Fixture {
    fn a(&self) -> &WorkspaceSeed {
        &self.ws.a
    }

    fn workspace_group(&self) -> String {
        format!("workspace:{}", self.a().workspace_id)
    }

    async fn snapshot(&self, groups: &str) -> Snapshot {
        bootstrap(&self.pool, self.a(), self.a().admin_uuid, groups).await
    }
}

#[actix_web::test]
async fn a_ticket_group_carries_the_ticket_and_everything_on_it() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let group = format!("ticket:{}", f.t1);
    let s = f.snapshot(&group).await;

    assert_eq!(s.granted, vec![group.clone()]);
    assert_eq!(s.of("ticket"), keys(&[f.t1]));
    assert_eq!(
        s.of("comment"),
        f.comments,
        "every comment on the ticket, internal ones included, and no other ticket's"
    );
    assert_eq!(s.of("attachment"), f.attachments);
    assert_eq!(s.of("ticket_asset"), BTreeSet::from([pair(f.t1, f.asset)]));
    assert_eq!(
        s.of("linked_ticket"),
        BTreeSet::from([pair(f.t1, f.t2)]),
        "the links the ticket is the subject of"
    );
    assert_eq!(s.of("project_ticket"), BTreeSet::from([pair(f.p, f.t1)]));
    assert_eq!(
        s.of("project"),
        keys(&[f.p]),
        "the projects the ticket is in"
    );
    for model in ["documentation_collection", "documentation_page"] {
        assert!(
            s.of(model).is_empty(),
            "{model} rows ride the workspace group only"
        );
    }
}

#[actix_web::test]
async fn a_project_group_carries_the_project_its_links_and_tickets() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let group = format!("project:{}", f.p);
    let s = f.snapshot(&group).await;

    assert_eq!(s.granted, vec![group.clone()]);
    assert_eq!(s.of("project"), keys(&[f.p]));
    assert_eq!(
        s.of("project_ticket"),
        BTreeSet::from([pair(f.p, f.t1), pair(f.p, f.t2)])
    );
    assert_eq!(s.of("ticket"), keys(&[f.t1, f.t2]));
    for model in [
        "comment",
        "attachment",
        "ticket_asset",
        "linked_ticket",
        "documentation_collection",
        "documentation_page",
    ] {
        assert!(s.of(model).is_empty(), "{model} rows are not the project's");
    }
}

#[actix_web::test]
async fn the_workspace_group_carries_every_record_in_the_workspace() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let group = f.workspace_group();
    let s = f.snapshot(&group).await;

    assert_eq!(s.granted, vec![group.clone()]);
    assert_eq!(
        s.of("ticket"),
        keys(&[f.t1, f.t2, f.t3]),
        "open and closed, in a project or not"
    );
    assert_eq!(s.of("project"), keys(&[f.p, f.q]));
    assert_eq!(
        s.of("project_ticket"),
        BTreeSet::from([pair(f.p, f.t1), pair(f.p, f.t2)])
    );
    assert_eq!(s.of("documentation_collection"), keys(&[f.collection]));
    assert_eq!(
        s.of("documentation_page"),
        f.pages,
        "pages of every status, the trash included"
    );
    for model in ["comment", "attachment", "ticket_asset", "linked_ticket"] {
        assert!(
            s.of(model).is_empty(),
            "{model} rows ride ticket groups only"
        );
    }
}

/// Workflow states, assets and people go with every snapshot, whatever the
/// group, so a ticket's or a project's snapshot carries the same set as the
/// workspace's. People are the workspace's: the viewer, its members, and
/// anyone its records name, even someone from another workspace; never
/// someone who belongs only elsewhere.
#[actix_web::test]
async fn every_group_carries_the_states_assets_and_people() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let a = f.a();
    let b = &f.ws.b;
    let groups = [
        f.workspace_group(),
        format!("project:{}", f.p),
        format!("ticket:{}", f.t1),
    ];

    for group in &groups {
        let s = f.snapshot(group).await;
        assert_eq!(s.of("workflow_state"), f.states, "{group}: every state");
        assert_eq!(s.of("asset"), keys(&[f.asset]), "{group}: every asset");

        let people = s.of("user");
        for (uuid, who) in [
            (a.admin_uuid, "the viewer"),
            (a.member_uuid, "A's member"),
            (f.requester_from_b, "the requester of A's ticket, from B"),
        ] {
            assert!(people.contains(&uuid.to_string()), "{group}: {who}");
        }
        for (uuid, who) in [(b.admin_uuid, "B's admin"), (b.member_uuid, "B's member")] {
            assert!(
                !people.contains(&uuid.to_string()),
                "{group}: {who} belongs only to B"
            );
        }
    }

    let first = f.snapshot(&groups[0]).await;
    for group in &groups[1..] {
        let s = f.snapshot(group).await;
        for model in ["workflow_state", "asset", "user"] {
            assert_eq!(
                s.of(model),
                first.of(model),
                "{group} carries the same {model} rows as the workspace group"
            );
        }
    }
}

#[actix_web::test]
async fn a_bootstrap_for_several_groups_carries_the_union() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let groups = [
        f.workspace_group(),
        format!("project:{}", f.p),
        format!("ticket:{}", f.t1),
    ];

    let mut union: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for group in &groups {
        for (model, rows) in f.snapshot(group).await.rows {
            union.entry(model).or_default().extend(rows);
        }
    }
    let together = f.snapshot(&groups.join(",")).await;

    assert_eq!(together.granted, groups.to_vec());
    assert_eq!(together.rows, union);
}
