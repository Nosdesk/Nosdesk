//! `nosdesk-cli workspace`: an export of a seeded workspace imports into an
//! untouched one through a workspace import job with the operator origin,
//! the running server reindexes it when asked, and the job resets; a dry run
//! previews without writing; refusals stop the import and leave no live job
//! behind. Drives `services::workspace_transfer`, which the CLI's commands
//! call.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::models::NewWorkspace;
use backend::repository::workspaces::{add_membership, create_workspace, SeatWriteAuthority};
use backend::services::search::{SearchQuery, SearchService};
use backend::services::seed::{seed_getting_started, seed_workspace_defaults};
use backend::services::workspace_transfer::{self as transfer, ImportRequest};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::storage::{LocalStorage, Storage, WorkspaceScopedStorage};

use crate::common::{insert_plain_user, seed_two_workspaces, TestDb, TestPool};

const REF: &str = "test:workspace_cli";
const PASSWORD: &str = "cli-password";

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

fn all_users(pool: &TestPool) -> i64 {
    count(
        pool,
        "SELECT count(*)::bigint AS n FROM users WHERE $1 = $1",
        0,
    )
}

/// Storage of this test's own, for the target.
fn private_storage() -> Arc<dyn Storage> {
    static ROOT: OnceLock<tempfile::TempDir> = OnceLock::new();
    let root = ROOT.get_or_init(|| tempfile::tempdir().expect("storage root"));
    let dir = root.path().join(Uuid::now_v7().to_string());
    Arc::new(LocalStorage::new(
        dir.to_string_lossy().into_owned(),
        "/files".into(),
    ))
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

/// A seeded workspace with a ticket, a staff member with an email and an
/// attachment file, exported through the CLI's export to a file. The
/// source's emails are then moved aside, so the import creates its people.
struct Source {
    ticket_title: String,
    file: tempfile::NamedTempFile,
}

async fn source(pool: &TestPool, password: Option<&str>) -> Source {
    let seed = {
        let mut conn = pool.get().expect("conn");
        random_workspace_id(&mut conn);
        seed_two_workspaces(&mut conn).a
    };
    let ticket_title = format!("Projector smoking {}", Uuid::new_v4().simple());
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
        .bind::<Text, _>(format!("staff-{}@acme.example", Uuid::new_v4().simple()))
        .execute(&mut conn)
        .expect("staff email");
    }
    let storage = crate::common::test_storage();
    WorkspaceScopedStorage::arc(storage.clone(), seed.workspace_id)
        .put_file(
            b"%PDF-1.4 cli",
            "tickets/1/attachments/manual.pdf",
            "application/pdf",
        )
        .await
        .expect("source file");

    // The CLI resolves the workspace by slug as well as id.
    let ws = transfer::resolve_workspace(pool, &seed.slug)
        .await
        .expect("resolve by slug");
    assert_eq!(ws.id, seed.workspace_id);
    let file = tempfile::NamedTempFile::new().expect("out file");
    let size = transfer::export(pool, storage, ws.id, password, file.path())
        .await
        .expect("export");
    assert!(size > 0);
    let head = std::fs::read(file.path()).expect("read export");
    assert_eq!(
        &head[..4],
        if password.is_some() {
            b"NODB"
        } else {
            b"PK\x03\x04"
        }
    );
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(file.path())
            .expect("meta")
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "an export is readable by its owner only"
    );

    diesel::sql_query("DELETE FROM user_emails WHERE user_uuid = $1")
        .bind::<SqlUuid, _>(seed.admin_uuid)
        .execute(&mut pool.get().expect("conn"))
        .expect("move the source's email aside");
    Source { ticket_title, file }
}

/// A workspace as provisioning leaves it: seeded, its owner, the welcome
/// page.
fn target(pool: &TestPool) -> (i32, String) {
    let mut conn = pool.get().expect("conn");
    random_workspace_id(&mut conn);
    let tag = Uuid::new_v4().simple().to_string()[..8].to_string();
    let ws = create_workspace(
        &mut conn,
        &NewWorkspace {
            uuid: Uuid::now_v7(),
            slug: format!("cli-{tag}"),
            name: format!("CLI Target {tag}"),
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
    (ws.id, ws.slug)
}

fn request(workspace_id: i32, src: &Source, password: Option<&str>) -> ImportRequest {
    ImportRequest {
        workspace_id,
        archive: src.file.path().to_path_buf(),
        password: password.map(str::to_string),
        // The source is still on this server.
        regenerate_uuids: true,
    }
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

fn job_states(pool: &TestPool, workspace_id: i32) -> Vec<String> {
    #[derive(QueryableByName)]
    struct State {
        #[diesel(sql_type = Text)]
        state: String,
    }
    diesel::sql_query(
        "SELECT state FROM workspace_import_jobs WHERE workspace_id = $1 ORDER BY created_at",
    )
    .bind::<Integer, _>(workspace_id)
    .load::<State>(&mut pool.get().expect("conn"))
    .expect("job states")
    .into_iter()
    .map(|s| s.state)
    .collect()
}

/// Export, check, import, then reset: the job pipeline end to end, as the
/// operator runs it.
#[actix_web::test]
async fn an_export_imports_into_an_untouched_workspace_through_a_job() {
    let db = TestDb::new();
    let pool = db.pool_with_size(6);
    let src = source(&pool, None).await;
    let (target_id, target_slug) = target(&pool);
    let rt = transfer::runtime(pool.clone(), private_storage());
    let origin = transfer::operator();

    // The server, holding the index, listens for the CLI's reindex request.
    let index_dir = tempfile::tempdir().expect("index dir");
    let search = Arc::new(SearchService::new(index_dir.path(), &pool).expect("init search"));
    let shutdown = tokio_util::sync::CancellationToken::new();
    let listener = backend::services::search::reindex_requests::spawn(
        db.url().to_string(),
        search.clone(),
        shutdown.clone(),
    );

    let ws = transfer::resolve_workspace(&pool, &target_id.to_string())
        .await
        .expect("resolve by id");
    assert_eq!(ws.slug, target_slug);

    // Check: a job, previewed.
    let (job, preview) = transfer::check(&rt, &origin, &request(target_id, &src, None))
        .await
        .expect("check");
    assert_eq!(job.state, "previewed");
    assert_eq!(job.runner, "operator");
    assert!(preview.refused.is_none());
    let shown = transfer::render_preview(&preview.report);
    assert!(shown.contains("tickets"), "{shown}");
    assert!(shown.contains("new account"), "{shown}");
    assert_eq!(
        count_in(&pool, "tickets", target_id),
        0,
        "a check writes no rows"
    );

    // Import, following its stages.
    let mut stages = Vec::new();
    let job = transfer::import(&rt, &origin, target_id, job.id, None, |j| {
        stages.push(j.stage.clone().unwrap_or_default())
    })
    .await
    .expect("import");
    assert_eq!(job.state, "imported", "{:?}", job.error);
    assert_eq!(stages.first().map(String::as_str), Some("rows"));
    assert!(job.confirmed_at.is_some());
    assert_eq!(count_in(&pool, "tickets", target_id), 1);
    let created: Vec<Uuid> = job.created_people.iter().flatten().copied().collect();
    assert!(!created.is_empty());
    assert!(job.files_restored >= 1);
    let restored = WorkspaceScopedStorage::arc(rt.storage.clone(), target_id);
    assert!(restored
        .get_file("tickets/1/attachments/manual.pdf")
        .await
        .is_ok_and(|b| b == b"%PDF-1.4 cli"));
    let report = transfer::render_job(&job);
    assert!(report.contains("People created: "), "{report}");
    assert!(report.contains("Files: "), "{report}");

    // The job is the app's: status finds it by id alone.
    let found = transfer::find_job(&pool, job.id).await.expect("status");
    assert_eq!(found.workspace_id, target_id);
    assert_eq!(found.state, "imported");

    // The listening server reindexed the workspace.
    let word = src.ticket_title.rsplit(' ').next().expect("word");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        search.reload().expect("reload");
        if !search.is_reindexing_workspace(target_id) && ticket_hits(&search, target_id, word) == 1
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the imported ticket isn't indexed"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Reset: the workspace untouched again, the people gone.
    let job = transfer::reset(&rt, &origin, target_id, job.id)
        .await
        .expect("reset");
    assert_eq!(job.state, "reset");
    assert_eq!(count_in(&pool, "tickets", target_id), 0);
    let left = diesel::sql_query("SELECT count(*)::bigint AS n FROM users WHERE uuid = ANY($1)")
        .bind::<diesel::sql_types::Array<SqlUuid>, _>(&created)
        .get_result::<Count>(&mut pool.get().expect("conn"))
        .expect("count")
        .n;
    assert_eq!(left, 0);

    shutdown.cancel();
    let _ = listener.await;
}

/// A dry run prints the preview and writes nothing: no job, no rows, no
/// people, no files.
#[actix_web::test]
async fn a_dry_run_writes_nothing() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool, Some(PASSWORD)).await;
    let (target_id, _) = target(&pool);
    let rt = transfer::runtime(pool.clone(), private_storage());
    let origin = transfer::operator();
    let users_before = all_users(&pool);

    // Sealed: the password is needed.
    let refused = transfer::dry_run(&rt, &origin, &request(target_id, &src, None))
        .await
        .expect_err("no password");
    assert_eq!(refused.code, "workspace_import.password_required");
    let refused = transfer::dry_run(&rt, &origin, &request(target_id, &src, Some("wrong")))
        .await
        .expect_err("wrong password");
    assert_eq!(refused.code, "workspace_import.password_wrong");

    let preview = transfer::dry_run(&rt, &origin, &request(target_id, &src, Some(PASSWORD)))
        .await
        .expect("dry run");
    assert!(preview.refused.is_none(), "{:?}", preview.refused);
    let shown = transfer::render_preview(&preview.report);
    assert!(shown.contains("sealed:   yes"), "{shown}");
    assert!(shown.contains("tickets"), "{shown}");
    assert!(shown.contains("People ("), "{shown}");
    assert!(
        preview.report["plan"]["counts"]["created"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    );

    assert!(job_states(&pool, target_id).is_empty(), "no job");
    assert_eq!(count_in(&pool, "tickets", target_id), 0);
    assert_eq!(all_users(&pool), users_before);
    assert!(WorkspaceScopedStorage::arc(rt.storage.clone(), target_id)
        .list_prefix("")
        .await
        .expect("list")
        .is_empty());
}

/// A workspace that holds data refuses with what it holds and keeps no
/// live job; a workspace with an import under way refuses another, dry run
/// or not.
#[actix_web::test]
async fn refusals_stop_the_import() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let src = source(&pool, None).await;
    let rt = transfer::runtime(pool.clone(), private_storage());
    let origin = transfer::operator();

    // Not untouched.
    let (touched, _) = target(&pool);
    diesel::sql_query(
        "INSERT INTO tickets (workspace_id, title, workflow_state_id) \
         VALUES ($1, 'Already here', (SELECT id FROM workflow_states WHERE workspace_id = $1 \
                          ORDER BY id LIMIT 1))",
    )
    .bind::<Integer, _>(touched)
    .execute(&mut pool.get().expect("conn"))
    .expect("ticket");
    let refused = transfer::check(&rt, &origin, &request(touched, &src, None))
        .await
        .expect_err("touched");
    assert_eq!(refused.code, "workspace_not_empty");
    assert!(
        transfer::render_refusal(&refused).contains("holds 1 tickets"),
        "{}",
        transfer::render_refusal(&refused)
    );
    assert_eq!(
        job_states(&pool, touched),
        vec!["reset".to_string()],
        "the refused job is discarded, not left live"
    );
    let refused = transfer::dry_run(&rt, &origin, &request(touched, &src, None))
        .await
        .expect_err("touched, dry run");
    assert_eq!(refused.code, "workspace_not_empty");

    // An import under way.
    let (busy, _) = target(&pool);
    let (job, _) = transfer::check(&rt, &origin, &request(busy, &src, None))
        .await
        .expect("first check");
    for dry in [true, false] {
        let req = request(busy, &src, None);
        let refused = if dry {
            transfer::dry_run(&rt, &origin, &req).await
        } else {
            transfer::check(&rt, &origin, &req).await.map(|(_, p)| p)
        }
        .expect_err("in progress");
        assert_eq!(refused.code, "import_in_progress");
        assert_eq!(refused.args["job_id"], job.id.to_string());
    }

    // Unknown workspace.
    let refused = transfer::resolve_workspace(&pool, "no-such-workspace")
        .await
        .expect_err("unknown");
    assert_eq!(refused.code, "workspace_not_found");
}
