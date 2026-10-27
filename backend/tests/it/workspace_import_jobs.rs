//! Workspace imports as jobs: the owner uploads an export, the check
//! previews it without writing, confirm imports it in the background and
//! reindexes it, and reset takes the workspace back to untouched and removes
//! the people the import created. One live import per workspace; only the
//! owner (or a platform admin, on the admin routes) reaches them; a stalled
//! job is put back; the prune drops archives after 30 days; and the CSV
//! importer's orphan sweep leaves a parked archive alone.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test::{self as http_test, TestRequest};
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Text, Uuid as SqlUuid};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewWorkspace};
use backend::repository::workspaces::{
    add_membership, create_workspace, data_fence, DataFence, SeatWriteAuthority,
};
use backend::services::search::{SearchQuery, SearchService};
use backend::services::seed::{seed_getting_started, seed_workspace_defaults};
use backend::services::workspace_archive::apply;
use backend::sync::actor::ActorContext;
use backend::sync::session::{with_actor_bypass_context, with_actor_context};
use backend::utils::storage::{LocalStorage, Storage, WorkspaceScopedStorage};

use crate::common::{insert_plain_user, seed_two_workspaces, TestDb, TestPool};
use crate::workspace_export_hygiene::call_admin_route;

const REF: &str = "test:workspace_import_jobs";
const BOUNDARY: &str = "nosdesk-import-jobs-boundary";
const PASSWORD: &str = "jobs-password";

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count(pool: &TestPool, sql: &str, workspace_id: i32) -> i64 {
    diesel::sql_query(sql)
        .bind::<Integer, _>(workspace_id)
        .get_result::<Count>(&mut pool.get().expect("conn"))
        .expect("count")
        .n
}

fn count_in(pool: &TestPool, table: &str, workspace_id: i32) -> i64 {
    count(
        pool,
        &format!("SELECT count(*)::bigint AS n FROM {table} WHERE workspace_id = $1"),
        workspace_id,
    )
}

fn users_named(pool: &TestPool, uuids: &[Uuid]) -> i64 {
    diesel::sql_query("SELECT count(*)::bigint AS n FROM users WHERE uuid = ANY($1)")
        .bind::<diesel::sql_types::Array<SqlUuid>, _>(uuids)
        .get_result::<Count>(&mut pool.get().expect("conn"))
        .expect("count users")
        .n
}

/// Storage of this test's own, so what it finds there is only what it wrote.
fn private_storage() -> Arc<dyn Storage> {
    static ROOT: OnceLock<tempfile::TempDir> = OnceLock::new();
    let root = ROOT.get_or_init(|| tempfile::tempdir().expect("storage root"));
    let dir = root.path().join(Uuid::now_v7().to_string());
    Arc::new(LocalStorage::new(
        dir.to_string_lossy().into_owned(),
        "/files".into(),
    ))
}

/// Who a request is from.
#[derive(Clone, Copy)]
enum Caller {
    Member(Uuid),
    PlatformAdmin(Uuid),
}

/// What a request runs against.
struct Env {
    pool: TestPool,
    storage: Arc<dyn Storage>,
    search: Option<Arc<SearchService>>,
}

impl Env {
    async fn call(
        &self,
        workspace_id: i32,
        caller: Caller,
        req: TestRequest,
    ) -> (StatusCode, Value) {
        let (user, role) = match caller {
            Caller::Member(u) => (u, "user"),
            Caller::PlatformAdmin(u) => (u, "platform_admin"),
        };
        let now = chrono::Utc::now().timestamp();
        let claims = Claims {
            sub: user.to_string(),
            name: "Importer".to_string(),
            email: "importer@example.com".to_string(),
            platform_role: role.to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (now + 3600) as usize,
            iat: now as usize,
        };
        let workspace = WorkspaceContext {
            workspace_id,
            workspace_uuid: Uuid::now_v7(),
            slug: format!("ws-{workspace_id}"),
            name: format!("ws-{workspace_id}"),
            organisation_id: None,
            custom_domain: None,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user, Some(corr)).with_workspace(workspace_id);
        let mut app = App::new()
            .app_data(web::Data::new(self.pool.clone()))
            .app_data(web::Data::new(self.storage.clone()));
        if let Some(search) = &self.search {
            app = app.app_data(web::Data::new(search.clone()));
        }
        let app = http_test::init_service(
            app.wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::workspace_import::config)),
        )
        .await;
        let resp = http_test::call_service(&app, req.to_request()).await;
        let status = resp.status();
        let body = http_test::read_body(resp).await;
        let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
        (status, json)
    }

    /// Poll the job until it leaves `checking` and `importing`.
    async fn settled(&self, workspace_id: i32, caller: Caller, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let (status, job) = self
                .call(
                    workspace_id,
                    caller,
                    TestRequest::get().uri(&format!("/api/workspace/imports/{id}")),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{job}");
            if !matches!(job["state"].as_str(), Some("checking" | "importing")) {
                return job;
            }
            assert!(Instant::now() < deadline, "job still running: {job}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// A multipart upload of `archive` to `uri`.
fn upload(uri: &str, archive: &[u8], fields: &[(&str, &str)]) -> TestRequest {
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"archive\"; \
             filename=\"acme.nosdesk\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(archive);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    TestRequest::post()
        .uri(uri)
        .insert_header((
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        ))
        .set_payload(body)
}

/// Move the workspace sequence to a random id, so a workspace's files
/// (under `ws/{id}/` in the binary's shared storage) are its own.
fn random_workspace_id(conn: &mut crate::common::TestPooledConn) {
    let id = 1_000_000 + (Uuid::new_v4().as_u128() % 1_000_000_000) as i64;
    diesel::sql_query("SELECT setval(pg_get_serial_sequence('public.workspaces', 'id'), $1)")
        .bind::<BigInt, _>(id)
        .execute(conn)
        .expect("workspace id");
}

/// The source: a workspace with a ticket, a staff member with an email and
/// an attachment file, exported. The source's emails are then moved aside,
/// so the import creates its people rather than finding them here.
struct Source {
    ticket_title: String,
    archive: Vec<u8>,
}

async fn source(pool: &TestPool, password: Option<&str>) -> Source {
    let seed = {
        let mut conn = pool.get().expect("conn");
        random_workspace_id(&mut conn);
        seed_two_workspaces(&mut conn).a
    };
    let ticket_title = format!("Projector smoking {}", Uuid::new_v4().simple());
    let email = format!("staff-{}@acme.example", Uuid::new_v4().simple());
    {
        let mut conn = pool.get().expect("conn");
        diesel::sql_query(
            "INSERT INTO tickets (workspace_id, title, workflow_state_id) \
             VALUES ($1, $2, (SELECT id FROM workflow_states WHERE workspace_id = $1 \
                              ORDER BY id LIMIT 1))",
        )
        .bind::<Integer, _>(seed.workspace_id)
        .bind::<Text, _>(&ticket_title)
        .execute(&mut conn)
        .expect("ticket");
        diesel::sql_query(
            "INSERT INTO user_emails (user_uuid, email, is_primary, is_verified) \
             VALUES ($1, $2, true, true)",
        )
        .bind::<SqlUuid, _>(seed.admin_uuid)
        .bind::<Text, _>(&email)
        .execute(&mut conn)
        .expect("staff email");
    }
    WorkspaceScopedStorage::arc(crate::common::test_storage(), seed.workspace_id)
        .put_file(
            b"%PDF-1.4 jobs",
            "tickets/1/attachments/manual.pdf",
            "application/pdf",
        )
        .await
        .expect("source file");
    let mut body = json!({});
    if let Some(password) = password {
        body["password"] = json!(password);
    }
    let (status, bytes) = call_admin_route(
        pool,
        &seed,
        TestRequest::post()
            .uri(&format!(
                "/api/admin/workspaces/{}/export",
                seed.workspace_id
            ))
            .set_json(body),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    diesel::sql_query("DELETE FROM user_emails WHERE user_uuid = $1")
        .bind::<SqlUuid, _>(seed.admin_uuid)
        .execute(&mut pool.get().expect("conn"))
        .expect("move the source's email aside");
    Source {
        ticket_title,
        archive: bytes.to_vec(),
    }
}

/// A self-hosted workspace as setup leaves it: seeded, its owner, the
/// welcome page.
struct Target {
    id: i32,
    owner: Uuid,
}

fn target(pool: &TestPool) -> Target {
    let mut conn = pool.get().expect("conn");
    random_workspace_id(&mut conn);
    let tag = Uuid::new_v4().simple().to_string()[..8].to_string();
    let ws = create_workspace(
        &mut conn,
        &NewWorkspace {
            uuid: Uuid::now_v7(),
            slug: format!("jobs-{tag}"),
            name: format!("Jobs Target {tag}"),
            seat_limit: None,
        },
    )
    .expect("create workspace");
    let owner = insert_plain_user(&mut conn, "Target Owner");
    let actor = ActorContext::system(REF).with_workspace(ws.id);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        seed_workspace_defaults(c, None)?;
        add_membership(c, ws.id, owner, "owner", SeatWriteAuthority::Product)?;
        seed_getting_started(c, owner)
    })
    .expect("set up the target");
    Target { id: ws.id, owner }
}

fn add_member(pool: &TestPool, workspace_id: i32, role: &str) -> Uuid {
    let mut conn = pool.get().expect("conn");
    let user = insert_plain_user(&mut conn, "Another Member");
    let actor = ActorContext::system(REF).with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        add_membership(c, workspace_id, user, role, SeatWriteAuthority::Product)
    })
    .expect("add member");
    user
}

fn untouched(pool: &TestPool, workspace_id: i32) -> Vec<apply::Found> {
    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        apply::untouched(c, workspace_id, &[])
            .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    })
    .expect("untouched check")
}

fn ticket_hits(search: &SearchService, workspace_id: i32, q: &str) -> usize {
    let query = SearchQuery {
        q: q.to_string(),
        limit: 50,
        types: Some("ticket".to_string()),
        sort: None,
        author: None,
    };
    search
        .search(&query, true, workspace_id as i64)
        .expect("search")
        .results
        .len()
}

/// The whole flow: upload, a preview that writes nothing, confirm, the
/// imported workspace searchable, then a reset that leaves it untouched
/// and removes the people the import created.
#[actix_web::test]
async fn an_owner_uploads_checks_confirms_and_resets() {
    let db = TestDb::new();
    let pool = db.pool_with_size(6);
    let src = source(&pool, None).await;
    let t = target(&pool);
    let index_dir = tempfile::tempdir().expect("index dir");
    let search = Arc::new(SearchService::new(index_dir.path(), &pool).expect("init search"));
    search.reload().expect("reload");
    let env = Env {
        pool: pool.clone(),
        storage: private_storage(),
        search: Some(search.clone()),
    };
    let owner = Caller::Member(t.owner);
    let users_before = count(
        &pool,
        "SELECT count(*)::bigint AS n FROM users WHERE $1 = $1",
        0,
    );

    // Upload: parked, then checked in the background.
    let (status, job) = env
        .call(
            t.id,
            owner,
            upload(
                "/api/workspace/imports",
                &src.archive,
                &[("regenerate_uuids", "true")],
            ),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["state"], "checking");
    assert_eq!(job["archive"]["stored"], true);
    assert_eq!(job["runner"], "owner");
    let id = job["id"].as_str().expect("id").to_string();
    let parked = WorkspaceScopedStorage::arc(env.storage.clone(), t.id);
    assert!(parked
        .list_prefix("workspace-imports/")
        .await
        .expect("list")
        .contains(&format!("workspace-imports/{id}.nosdesk")));

    // The preview: the plan's report, people and counts; nothing written.
    let job = env.settled(t.id, owner, &id).await;
    assert_eq!(job["state"], "previewed", "{job}");
    assert_eq!(job["error"], Value::Null);
    let plan = &job["report"]["plan"];
    assert!(
        plan["counts"]["created"].as_u64().unwrap_or(0) >= 1,
        "{plan}"
    );
    assert!(plan["people"].as_array().is_some_and(|p| !p.is_empty()));
    assert!(job["report"]["source"]["nosdesk_version"].is_string());
    assert_eq!(job["actions"]["confirm"], true);
    assert_eq!(count_in(&pool, "tickets", t.id), 0);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*)::bigint AS n FROM users WHERE $1 = $1",
            0
        ),
        users_before
    );
    assert_eq!(
        data_fence(&mut pool.get().expect("conn"), t.id).expect("fence"),
        DataFence::default()
    );

    // Confirm: rows, search, files.
    let (status, job) = env
        .call(
            t.id,
            owner,
            TestRequest::post()
                .uri(&format!("/api/workspace/imports/{id}/confirm"))
                .set_json(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["state"], "importing");
    let job = env.settled(t.id, owner, &id).await;
    assert_eq!(job["state"], "imported", "{job}");
    assert_eq!(count_in(&pool, "tickets", t.id), 1);
    let created: Vec<Uuid> = {
        #[derive(QueryableByName)]
        struct People {
            #[diesel(sql_type = diesel::sql_types::Array<SqlUuid>)]
            created_people: Vec<Uuid>,
        }
        diesel::sql_query("SELECT created_people FROM workspace_import_jobs WHERE id = $1::uuid")
            .bind::<Text, _>(&id)
            .get_result::<People>(&mut pool.get().expect("conn"))
            .expect("job row")
            .created_people
    };
    assert!(!created.is_empty());
    assert_eq!(job["people_created"], created.len());
    assert_eq!(users_named(&pool, &created), created.len() as i64);
    assert!(job["files_restored"].as_i64().unwrap_or(0) >= 1, "{job}");
    assert!(parked
        .get_file("tickets/1/attachments/manual.pdf")
        .await
        .is_ok_and(|b| b == b"%PDF-1.4 jobs"));
    assert_eq!(
        job["archive"]["stored"], false,
        "the archive goes once imported"
    );
    assert!(parked
        .list_prefix("workspace-imports/")
        .await
        .expect("list")
        .is_empty());
    assert_eq!(
        job["sign_in"]["pending"], 1,
        "the staff member with an email"
    );
    assert!(data_fence(&mut pool.get().expect("conn"), t.id)
        .expect("fence")
        .epoch
        .is_some());

    // Reindexed for search.
    let word = src.ticket_title.rsplit(' ').next().expect("word");
    let deadline = Instant::now() + Duration::from_secs(60);
    while search.is_reindexing_workspace(t.id) {
        assert!(Instant::now() < deadline, "reindex still running");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    search.reload().expect("reload");
    assert_eq!(ticket_hits(&search, t.id, word), 1);

    // Reset: untouched again, the people the import created gone, the
    // owner still here.
    let (status, job) = env
        .call(
            t.id,
            owner,
            TestRequest::post().uri(&format!("/api/workspace/imports/{id}/reset")),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["state"], "reset");
    assert!(job["report"]["reset"]["people_removed"].as_u64() == Some(created.len() as u64));
    assert_eq!(count_in(&pool, "tickets", t.id), 0);
    assert_eq!(untouched(&pool, t.id), Vec::<apply::Found>::new());
    assert_eq!(count_in(&pool, "workflow_states", t.id), 7, "seeded again");
    assert_eq!(users_named(&pool, &created), 0);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*)::bigint AS n FROM workspace_members WHERE workspace_id = $1",
            t.id
        ),
        1,
        "only the owner"
    );
    assert!(parked
        .get_file("tickets/1/attachments/manual.pdf")
        .await
        .is_err());

    // A reset job is done with.
    let (status, body) = env
        .call(
            t.id,
            owner,
            TestRequest::post().uri(&format!("/api/workspace/imports/{id}/reset")),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "workspace_import.already_reset");
}

/// A second upload while one is live is refused, naming it; once the first
/// is discarded another can start. A sealed archive waits for its password.
#[actix_web::test]
async fn one_live_import_per_workspace() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool, Some(PASSWORD)).await;
    let t = target(&pool);
    let env = Env {
        pool: pool.clone(),
        storage: private_storage(),
        search: None,
    };
    let owner = Caller::Member(t.owner);

    let (status, job) = env
        .call(
            t.id,
            owner,
            upload("/api/workspace/imports", &src.archive, &[]),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["state"], "uploaded", "a sealed archive waits");
    assert_eq!(job["archive"]["sealed"], true);
    assert_eq!(job["error"]["code"], "workspace_import.password_required");
    let first = job["id"].as_str().expect("id").to_string();

    let (status, body) = env
        .call(
            t.id,
            owner,
            upload("/api/workspace/imports", &src.archive, &[]),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_in_progress");
    assert_eq!(body["args"]["job_id"], first.as_str());

    // A wrong password goes back to uploaded with the refusal.
    let (status, _) = env
        .call(
            t.id,
            owner,
            TestRequest::post()
                .uri(&format!("/api/workspace/imports/{first}/check"))
                .set_json(json!({ "password": "wrong", "regenerate_uuids": true })),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let job = env.settled(t.id, owner, &first).await;
    assert_eq!(job["state"], "uploaded");
    assert_eq!(job["error"]["code"], "workspace_import.password_wrong");

    // The right one previews.
    let (status, _) = env
        .call(
            t.id,
            owner,
            TestRequest::post()
                .uri(&format!("/api/workspace/imports/{first}/check"))
                .set_json(json!({ "password": PASSWORD })),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let job = env.settled(t.id, owner, &first).await;
    assert_eq!(job["state"], "previewed", "{job}");
    assert_eq!(job["regenerate_uuids"], true, "kept from the first check");

    // Confirm without the password is refused before anything runs.
    let (status, body) = env
        .call(
            t.id,
            owner,
            TestRequest::post()
                .uri(&format!("/api/workspace/imports/{first}/confirm"))
                .set_json(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "workspace_import.password_required");

    // Discard it; another upload starts.
    let (status, job) = env
        .call(
            t.id,
            owner,
            TestRequest::post().uri(&format!("/api/workspace/imports/{first}/reset")),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["state"], "reset");
    assert_eq!(job["archive"]["stored"], false);
    let (status, job) = env
        .call(
            t.id,
            owner,
            upload("/api/workspace/imports", &src.archive, &[]),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");

    // The index holds whatever path writes the row.
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::system(REF).with_workspace(t.id);
    let second = with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        diesel::sql_query(
            "INSERT INTO workspace_import_jobs (workspace_id, runner, state) \
             VALUES ($1, 'owner', 'previewed')",
        )
        .bind::<Integer, _>(t.id)
        .execute(c)
    });
    assert!(second.is_err(), "the partial unique index refuses a second");

    // The list shows both, newest first.
    let (status, list) = env
        .call(
            t.id,
            owner,
            TestRequest::get().uri("/api/workspace/imports"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().map(Vec::len), Some(2));
    assert_eq!(list[1]["id"], first.as_str());
    let (status, latest) = env
        .call(
            t.id,
            owner,
            TestRequest::get().uri("/api/workspace/imports/latest"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(latest["id"], list[0]["id"]);
}

/// A workspace that already has a ticket refuses at the check, naming it,
/// and the job waits to be checked again.
#[actix_web::test]
async fn a_touched_workspace_refuses_at_the_check() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool, None).await;
    let t = target(&pool);
    diesel::sql_query(
        "INSERT INTO tickets (workspace_id, title, workflow_state_id) \
         VALUES ($1, 'Already here', (SELECT id FROM workflow_states WHERE workspace_id = $1 \
                                      ORDER BY id LIMIT 1))",
    )
    .bind::<Integer, _>(t.id)
    .execute(&mut pool.get().expect("conn"))
    .expect("ticket");
    let env = Env {
        pool: pool.clone(),
        storage: private_storage(),
        search: None,
    };
    let owner = Caller::Member(t.owner);
    let (status, job) = env
        .call(
            t.id,
            owner,
            upload(
                "/api/workspace/imports",
                &src.archive,
                &[("regenerate_uuids", "true")],
            ),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let id = job["id"].as_str().expect("id").to_string();
    let job = env.settled(t.id, owner, &id).await;
    assert_eq!(job["state"], "uploaded");
    assert_eq!(job["error"]["code"], "workspace_not_empty");
    assert_eq!(
        job["error"]["args"]["found"],
        json!([{ "what": "tickets", "rows": 1 }])
    );
    assert_eq!(job["actions"]["check"], true);

    // Confirm isn't offered from there.
    let (status, body) = env
        .call(
            t.id,
            owner,
            TestRequest::post()
                .uri(&format!("/api/workspace/imports/{id}/confirm"))
                .set_json(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "workspace_import.wrong_state");
}

/// Only the owner reaches the tenant routes; another workspace's job is 404;
/// the admin routes take a platform admin.
#[actix_web::test]
async fn only_the_owner_or_a_platform_admin_reaches_imports() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool, None).await;
    let t = target(&pool);
    let other = target(&pool);
    let admin = add_member(&pool, t.id, "admin");
    let env = Env {
        pool: pool.clone(),
        storage: private_storage(),
        search: None,
    };

    let (status, _) = env
        .call(
            t.id,
            Caller::Member(admin),
            upload("/api/workspace/imports", &src.archive, &[]),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a workspace admin isn't the owner"
    );
    let (status, _) = env
        .call(
            t.id,
            Caller::Member(admin),
            TestRequest::get().uri("/api/workspace/imports"),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, job) = env
        .call(
            t.id,
            Caller::Member(t.owner),
            upload(
                "/api/workspace/imports",
                &src.archive,
                &[("regenerate_uuids", "true")],
            ),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    let id = job["id"].as_str().expect("id").to_string();
    let (status, body) = env
        .call(
            other.id,
            Caller::Member(other.owner),
            TestRequest::get().uri(&format!("/api/workspace/imports/{id}")),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "workspace_import.not_found");

    // The admin routes: a platform admin, any workspace.
    let (status, _) = env
        .call(
            other.id,
            Caller::Member(other.owner),
            TestRequest::get().uri(&format!("/api/admin/workspaces/{}/imports", t.id)),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let operator = Caller::PlatformAdmin(insert_plain_user(
        &mut pool.get().expect("conn"),
        "Operator",
    ));
    let (status, job) = env
        .call(
            other.id,
            operator,
            TestRequest::get().uri(&format!("/api/admin/workspaces/{}/imports/{id}", t.id)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{job}");
    assert_eq!(job["id"], id.as_str());
    let (status, body) = env
        .call(
            other.id,
            operator,
            TestRequest::get().uri("/api/admin/workspaces/2147483000/imports"),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "workspace_not_found");

    // An operator's upload into the other workspace runs as the operator.
    let (status, job) = env
        .call(
            t.id,
            operator,
            upload(
                &format!("/api/admin/workspaces/{}/imports", other.id),
                &src.archive,
                &[("regenerate_uuids", "true")],
            ),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{job}");
    assert_eq!(job["runner"], "operator");
    assert_eq!(job["workspace_id"], other.id);

    // Let both background checks finish before the database goes.
    env.settled(t.id, Caller::Member(t.owner), &id).await;
    let theirs = job["id"].as_str().expect("id").to_string();
    env.settled(other.id, Caller::Member(other.owner), &theirs)
        .await;
}

/// Insert a job row directly in `state`, as a stalled or old job would
/// stand, then apply `sql_set`.
fn insert_job(pool: &TestPool, workspace_id: i32, state: &str, sql_set: &str) -> Uuid {
    let id = Uuid::now_v7();
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::system(REF).with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        diesel::sql_query(
            "INSERT INTO workspace_import_jobs (id, workspace_id, runner, state, archive_key) \
             VALUES ($1, $2, 'owner', $3, 'workspace-imports/' || $1::text || '.nosdesk')",
        )
        .bind::<SqlUuid, _>(id)
        .bind::<Integer, _>(workspace_id)
        .bind::<Text, _>(state)
        .execute(c)?;
        if sql_set.is_empty() {
            return Ok(0);
        }
        diesel::sql_query(format!(
            "UPDATE workspace_import_jobs SET {sql_set} WHERE id = $1"
        ))
        .bind::<SqlUuid, _>(id)
        .execute(c)
    })
    .expect("job");
    id
}

fn state_of(pool: &TestPool, id: Uuid) -> (String, Option<String>) {
    #[derive(QueryableByName)]
    struct Row {
        #[diesel(sql_type = Text)]
        state: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
        code: Option<String>,
    }
    let row: Row = diesel::sql_query(
        "SELECT state::text AS state, error->>'code' AS code \
         FROM workspace_import_jobs WHERE id = $1",
    )
    .bind::<SqlUuid, _>(id)
    .get_result(&mut pool.get().expect("conn"))
    .expect("job");
    (row.state, row.code)
}

/// A job whose background run died is put back on the next read: a check
/// to `uploaded`, an import that never committed to `previewed`, one that
/// did to `failed` with retry files offered. A running heartbeat leaves no
/// audit row.
#[actix_web::test]
async fn a_stalled_job_is_put_back() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let t = target(&pool);
    let env = Env {
        pool: pool.clone(),
        storage: private_storage(),
        search: None,
    };
    let owner = Caller::Member(t.owner);
    let old = "updated_at = now() - interval '10 minutes'";

    let checking = insert_job(&pool, t.id, "checking", old);
    let job = env.settled(t.id, owner, &checking.to_string()).await;
    assert_eq!(job["state"], "uploaded");
    assert_eq!(job["error"]["code"], "workspace_import.check_stopped");
    // Done with that one.
    diesel::sql_query("DELETE FROM workspace_import_jobs WHERE id = $1")
        .bind::<SqlUuid, _>(checking)
        .execute(&mut pool.get().expect("conn"))
        .expect("delete");

    let importing = insert_job(&pool, t.id, "importing", old);
    let job = env.settled(t.id, owner, &importing.to_string()).await;
    assert_eq!(job["state"], "previewed");
    assert_eq!(job["error"]["code"], "workspace_import.import_stopped");
    diesel::sql_query("DELETE FROM workspace_import_jobs WHERE id = $1")
        .bind::<SqlUuid, _>(importing)
        .execute(&mut pool.get().expect("conn"))
        .expect("delete");

    let applied = insert_job(
        &pool,
        t.id,
        "importing",
        &format!("applied_at = now(), {old}"),
    );
    // Through the daily prune this time.
    backend::services::scheduled_jobs::prune_workspace_imports_before(
        &pool,
        env.storage.clone(),
        chrono::Utc::now() - chrono::Duration::days(30),
    )
    .await
    .expect("prune");
    assert_eq!(
        state_of(&pool, applied),
        ("failed".to_string(), Some("files_partial".to_string()))
    );
    let (_, job) = env
        .call(
            t.id,
            owner,
            TestRequest::get().uri(&format!("/api/workspace/imports/{applied}")),
        )
        .await;
    assert_eq!(job["actions"]["retry_files"], true);

    // A heartbeat changes updated_at alone: no audit row.
    let before = count(
        &pool,
        "SELECT count(*)::bigint AS n FROM audit_log \
         WHERE table_name = 'workspace_import_jobs' AND workspace_id = $1",
        t.id,
    );
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::system(REF).with_workspace(t.id);
    let beat = with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        backend::repository::workspace_import_jobs::heartbeat(c, applied, "failed", None, Some(3))
    })
    .expect("heartbeat");
    assert_eq!(beat, 1);
    let after = count(
        &pool,
        "SELECT count(*)::bigint AS n FROM audit_log \
         WHERE table_name = 'workspace_import_jobs' AND workspace_id = $1",
        t.id,
    );
    assert_eq!(after, before, "a heartbeat isn't audited");
}

/// After 30 days an unused upload goes with its archive, and an imported job
/// loses its archive and the people in its report and keeps its counts.
#[actix_web::test]
async fn the_prune_drops_archives_after_thirty_days() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let t = target(&pool);
    let storage = private_storage();
    let scoped = WorkspaceScopedStorage::arc(storage.clone(), t.id);
    let report = json!({ "plan": { "counts": { "created": 1 }, "people": [{ "name": "Ada" }] } })
        .to_string()
        .replace('\'', "''");
    let old = "updated_at = now() - interval '31 days'";
    let waiting = insert_job(&pool, t.id, "previewed", old);
    let imported = insert_job(
        &pool,
        t.id,
        "reset",
        &format!("applied_at = now() - interval '31 days', report = '{report}'::jsonb, {old}"),
    );
    let recent = insert_job(&pool, t.id, "failed", "");
    for id in [waiting, imported, recent] {
        scoped
            .put_file(
                b"NODB",
                &format!("workspace-imports/{id}.nosdesk"),
                "application/octet-stream",
            )
            .await
            .expect("archive");
    }

    backend::services::scheduled_jobs::prune_workspace_imports_before(
        &pool,
        storage.clone(),
        chrono::Utc::now() - chrono::Duration::days(30),
    )
    .await
    .expect("prune");

    let left = scoped
        .list_prefix("workspace-imports/")
        .await
        .expect("list");
    assert_eq!(left, vec![format!("workspace-imports/{recent}.nosdesk")]);
    assert_eq!(
        count(
            &pool,
            "SELECT count(*)::bigint AS n FROM workspace_import_jobs WHERE workspace_id = $1",
            t.id
        ),
        2,
        "the unused upload is gone"
    );
    #[derive(QueryableByName)]
    struct Pruned {
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Jsonb>)]
        report: Option<Value>,
        #[diesel(sql_type = diesel::sql_types::Bool)]
        pruned: bool,
    }
    let row: Pruned = diesel::sql_query(
        "SELECT report, pruned_at IS NOT NULL AS pruned FROM workspace_import_jobs WHERE id = $1",
    )
    .bind::<SqlUuid, _>(imported)
    .get_result(&mut pool.get().expect("conn"))
    .expect("job");
    assert!(row.pruned);
    let report = row.report.expect("report");
    assert_eq!(report["plan"]["counts"]["created"], 1);
    assert!(report["plan"].get("people").is_none());
}

/// The CSV importer's orphan sweep removes files under `imports/` that no
/// job keeps, and leaves an archive parked under `workspace-imports/`.
#[actix_web::test]
async fn the_csv_sweep_leaves_a_parked_archive_alone() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let t = target(&pool);
    let storage = private_storage();
    let scoped = WorkspaceScopedStorage::arc(storage.clone(), t.id);
    let job = Uuid::now_v7();
    let csv = format!("imports/{job}/upload.csv");
    let parked = format!("workspace-imports/{job}.nosdesk");
    for key in [&csv, &parked] {
        scoped
            .put_file(b"x", key, "application/octet-stream")
            .await
            .expect("put");
    }
    backend::services::scheduled_jobs::sweep_import_files_in(
        &pool,
        storage.clone(),
        chrono::Utc::now() + chrono::Duration::days(1),
    )
    .await
    .expect("sweep");
    assert!(
        scoped.get_file(&csv).await.is_err(),
        "the orphan CSV upload goes"
    );
    assert!(scoped.get_file(&parked).await.is_ok(), "the archive stays");
}

/// Invites need an import that created someone.
#[actix_web::test]
async fn invites_wait_for_an_import() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let t = target(&pool);
    let env = Env {
        pool: pool.clone(),
        storage: private_storage(),
        search: None,
    };
    let id = insert_job(&pool, t.id, "previewed", "");
    let (status, body) = env
        .call(
            t.id,
            Caller::Member(t.owner),
            TestRequest::post()
                .uri(&format!("/api/workspace/imports/{id}/invite"))
                .set_json(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_not_committed");
}
