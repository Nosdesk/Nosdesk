//! The documentation import's commit: the pages it writes (tree, order,
//! links, embeds, images, first revision, sync events, drafts), pages
//! imported before, a failed commit, one whose process died, and images
//! linked from the web.

use actix_web::http::StatusCode;
use actix_web::test::TestRequest;
use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::sync::session::run_in_workspace;
use backend::utils::storage::{Storage, WorkspaceScopedStorage};

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::import_workspaces;
use crate::imports_docs_flow::{
    call_docs, checked, committed, handbook, id_of, new_collection, on_job, page_count,
    pages_request, private_storage, uploaded, zip_of, PNG,
};
use std::sync::Arc;

const REF: &str = "test:imports_docs_commit";

#[derive(QueryableByName, Debug)]
struct PageRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = SqlUuid)]
    uuid: Uuid,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Text)]
    status: String,
    #[diesel(sql_type = diesel::sql_types::Bool)]
    is_public: bool,
    #[diesel(sql_type = Nullable<Integer>)]
    parent_id: Option<i32>,
    #[diesel(sql_type = Nullable<Integer>)]
    display_order: Option<i32>,
    #[diesel(sql_type = Nullable<Integer>)]
    collection_id: Option<i32>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Bytea>)]
    yjs_document: Option<Vec<u8>>,
    #[diesel(sql_type = diesel::sql_types::Timestamp)]
    updated_at: chrono::NaiveDateTime,
}

/// The workspace's pages, oldest first.
fn pages(pool: &TestPool, ws: &WorkspaceSeed) -> Vec<PageRow> {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "SELECT p.id, p.uuid, p.title, p.status::text AS status, p.is_public, p.parent_id, \
             p.display_order, cp.collection_id, p.yjs_document, p.updated_at::timestamp AS updated_at \
             FROM documentation_pages p \
             LEFT JOIN documentation_collection_pages cp ON cp.page_id = p.id \
             ORDER BY p.id",
        )
        .load::<PageRow>(c)
    })
    .expect("pages")
}

fn by_title<'a>(pages: &'a [PageRow], title: &str) -> &'a PageRow {
    pages
        .iter()
        .find(|p| p.title == title)
        .unwrap_or_else(|| panic!("no page {title}: {pages:?}"))
}

/// A page's body as ProseMirror JSON.
fn body(page: &PageRow) -> serde_json::Value {
    backend::services::docs_markdown::encode::decode::to_json(
        page.yjs_document.as_deref().expect("a body"),
    )
}

/// Every value of `key` anywhere in `value`.
fn all_of(value: &serde_json::Value, key: &str, out: &mut Vec<serde_json::Value>) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                if k == key {
                    out.push(v.clone());
                }
                all_of(v, key, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| all_of(v, key, out)),
        _ => {}
    }
}

fn images_of(page: &PageRow) -> Vec<String> {
    let mut srcs = Vec::new();
    all_of(&body(page), "src", &mut srcs);
    srcs.into_iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn hrefs_of(page: &PageRow) -> Vec<String> {
    let mut hrefs = Vec::new();
    all_of(&body(page), "href", &mut hrefs);
    hrefs
        .into_iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn scoped(storage: &Arc<dyn Storage>, ws: &WorkspaceSeed) -> WorkspaceScopedStorage {
    WorkspaceScopedStorage::new(storage.clone(), ws.workspace_id)
}

/// Every image stored for a page, in this test's own storage.
async fn collab_files(storage: &Arc<dyn Storage>, ws: &WorkspaceSeed) -> Vec<String> {
    let mut files = scoped(storage, ws)
        .list_prefix("collab/doc/")
        .await
        .unwrap_or_default();
    files.sort();
    files
}

fn count(pool: &TestPool, ws: &WorkspaceSeed, sql: &str) -> i64 {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(sql).get_result::<Count>(c)
    })
    .expect(sql)
    .n
}

/// A commit writes the tree as drafts nobody outside staff sees, parents
/// first and in order, in a new collection; links go to the new pages' ids;
/// archive and `data:` images are stored with the page that shows them and
/// no `data:` source is written; each page has its first revision; the
/// sync feed carries the new pages; the job records what it wrote.
#[actix_web::test]
async fn a_commit_writes_linked_draft_pages_in_order() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let storage = private_storage();
    let id = id_of(&uploaded(&pool, ws, "IT-Handbook.zip", &handbook()).await);
    checked(&pool, ws, &id, serde_json::json!({})).await;
    let job = committed(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["status"], "done", "{job}");
    assert_eq!(job["records_committed"], 4);
    assert_eq!(job["state"], "imported");

    let written = pages(&pool, ws);
    assert_eq!(written.len(), 4, "{written:?}");
    let start = by_title(&written, "Getting started");
    let network = by_title(&written, "Network");
    let vpn = by_title(&written, "VPN setup");
    let wifi = by_title(&written, "Wi-Fi");
    for page in &written {
        assert_eq!(page.status, "draft", "{page:?}");
        assert!(!page.is_public);
        assert!(page.collection_id.is_some());
        assert_eq!(page.collection_id, start.collection_id);
    }
    assert_eq!(start.parent_id, None);
    assert_eq!(network.parent_id, None);
    assert_eq!(vpn.parent_id, Some(network.id));
    assert_eq!(wifi.parent_id, Some(network.id));
    assert_eq!(start.display_order, Some(0));
    assert_eq!(network.display_order, Some(1));
    assert_eq!(vpn.display_order, Some(0));
    assert_eq!(wifi.display_order, Some(1));
    assert!(network.id < vpn.id, "parents first");

    #[derive(QueryableByName)]
    struct Name {
        #[diesel(sql_type = Text)]
        name: String,
    }
    let collection = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("SELECT name FROM documentation_collections WHERE id = $1")
            .bind::<Integer, _>(start.collection_id.unwrap())
            .get_result::<Name>(c)
    })
    .expect("collection");
    assert_eq!(collection.name, "IT-Handbook");

    // Links to pages in the import go to their ids.
    assert_eq!(hrefs_of(start), vec![format!("/documentation/{}", vpn.id)]);
    assert_eq!(hrefs_of(vpn), vec![format!("/documentation/{}", start.id)]);
    // The opening H1 that is the title is not repeated.
    assert!(!body(network).to_string().contains("\"Network\""));

    // Both images stored under the page, served from there, no data URI.
    let srcs = images_of(start);
    assert_eq!(srcs.len(), 2, "{srcs:?}");
    let prefix = format!("/api/files/collab/doc/{}/", start.uuid);
    for src in &srcs {
        assert!(src.starts_with(&prefix), "{src}");
    }
    assert!(!body(start).to_string().contains("data:"));
    let files = collab_files(&storage, ws).await;
    assert_eq!(files.len(), 2, "{files:?}");
    for src in &srcs {
        let key = src.trim_start_matches("/api/files/");
        assert_eq!(
            scoped(&storage, ws).get_file(key).await.expect("stored"),
            PNG
        );
    }

    // Revision 1 with where it came from.
    #[derive(QueryableByName)]
    struct Revision {
        #[diesel(sql_type = Integer)]
        revision_number: i32,
        #[diesel(sql_type = Nullable<Text>)]
        change_summary: Option<String>,
    }
    let revisions = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "SELECT revision_number, change_summary FROM documentation_revisions WHERE page_id = $1",
        )
        .bind::<Integer, _>(vpn.id)
        .load::<Revision>(c)
    })
    .expect("revisions");
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].revision_number, 1);
    assert_eq!(
        revisions[0].change_summary.as_deref(),
        Some("Imported from IT-Handbook.zip")
    );

    // The sync feed carries the pages and the collection.
    assert_eq!(
        count(
            &pool,
            ws,
            "SELECT count(*) AS n FROM sync_actions WHERE event_type = 'documentation_page.created'"
        ),
        4
    );
    assert_eq!(
        count(
            &pool,
            ws,
            "SELECT count(*) AS n FROM sync_actions WHERE event_type = 'documentation_collection.created'"
        ),
        1
    );

    // The record: each page, where it came from, as written.
    #[derive(QueryableByName)]
    struct Record {
        #[diesel(sql_type = Nullable<diesel::sql_types::Jsonb>)]
        record_ids: Option<serde_json::Value>,
    }
    let record = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("SELECT record_ids FROM import_jobs WHERE id = $1")
            .bind::<SqlUuid, _>(Uuid::parse_str(&id).unwrap())
            .get_result::<Record>(c)
    })
    .expect("record")
    .record_ids
    .expect("record ids");
    assert_eq!(record["pending"], false);
    let recorded = record["pages"].as_array().expect("pages");
    assert_eq!(recorded.len(), 4);
    let vpn_record = recorded
        .iter()
        .find(|p| p["id"] == vpn.id)
        .expect("vpn recorded");
    assert_eq!(vpn_record["source"], "network/vpn.md");
    use sha2::{Digest, Sha256};
    assert_eq!(
        vpn_record["hash"],
        hex::encode(Sha256::digest(vpn.yjs_document.as_ref().unwrap()))
    );
    let recorded_at: chrono::NaiveDateTime =
        serde_json::from_value(vpn_record["updated_at"].clone()).expect("updated_at");
    assert_eq!(recorded_at, vpn.updated_at);
    assert_eq!(record["collections_created"].as_array().unwrap().len(), 1);

    // A committed import's options can't change, and committing again
    // changes nothing.
    let (status, _, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        crate::imports_docs_flow::options_request(&id, serde_json::json!({}), true),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, again, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        crate::imports_docs_flow::commit_request(&id, serde_json::json!({})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(pages(&pool, ws).len(), 4);
}

/// `![[Note]]` in a vault embeds the imported page by its new uuid, and the
/// embed is recorded.
#[actix_web::test]
async fn embedded_pages_point_at_the_imported_page() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let zip = zip_of(&[
        (".obsidian/app.json", b"{}"),
        ("Home.md", b"Links to [[VPN]].\n\n![[VPN]]\n"),
        ("VPN.md", b"Connect with the client.\n"),
    ]);
    let id = id_of(&uploaded(&pool, ws, "vault.zip", &zip).await);
    let job = checked(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["summary"]["profile"], "obsidian", "{job}");
    committed(&pool, ws, &id, serde_json::json!({})).await;

    let written = pages(&pool, ws);
    let home = by_title(&written, "Home");
    let vpn = by_title(&written, "VPN");
    let mut uuids = Vec::new();
    all_of(&body(home), "documentUuid", &mut uuids);
    assert_eq!(uuids, vec![serde_json::json!(vpn.uuid.to_string())]);
    assert_eq!(hrefs_of(home), vec![format!("/documentation/{}", vpn.id)]);
    assert_eq!(
        count(
            &pool,
            ws,
            &format!(
                "SELECT count(*) AS n FROM documentation_page_embeddings \
                 WHERE source_page_id = {} AND target_page_id = {}",
                home.id, vpn.id
            )
        ),
        1
    );
}

/// Imported again into the same collection, the pages already there are
/// skipped by default; imported as copies, they are added after what the
/// collection holds. A page carrying the uuid of one here is found too.
#[actix_web::test]
async fn pages_imported_before_are_skipped_or_copied() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let collection = new_collection(&pool, ws, "Handbook");
    let into = serde_json::json!({ "collections": "existing", "collection_id": collection });

    let first = id_of(&uploaded(&pool, ws, "handbook.zip", &handbook()).await);
    checked(&pool, ws, &first, into.clone()).await;
    committed(&pool, ws, &first, serde_json::json!({})).await;
    assert_eq!(page_count(&pool, ws), 4);

    let second = id_of(&uploaded(&pool, ws, "handbook.zip", &handbook()).await);
    let job = checked(&pool, ws, &second, into.clone()).await;
    assert_eq!(job["summary"]["duplicates"], 4, "{job}");
    assert_eq!(job["summary"]["skipped"], 4);
    assert_eq!(job["summary"]["would_create"], 0);
    let (_, list, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        pages_request(&second, "?filter=skipped"),
        None,
    )
    .await;
    assert_eq!(list["total"], 4);
    assert_eq!(list["pages"][0]["duplicate"]["by"], "source", "{list}");
    let job = committed(&pool, ws, &second, serde_json::json!({})).await;
    assert_eq!(job["status"], "done");
    assert_eq!(job["records_committed"], 0);
    assert_eq!(page_count(&pool, ws), 4, "nothing written");

    // As copies: four more, the top-level ones after the first import's.
    let third = id_of(&uploaded(&pool, ws, "handbook.zip", &handbook()).await);
    checked(&pool, ws, &third, into.clone()).await;
    let job = committed(
        &pool,
        ws,
        &third,
        serde_json::json!({ "duplicates": "copy" }),
    )
    .await;
    assert_eq!(job["records_committed"], 4, "{job}");
    let written = pages(&pool, ws);
    assert_eq!(written.len(), 8);
    let roots: Vec<(&str, Option<i32>)> = written
        .iter()
        .filter(|p| p.parent_id.is_none())
        .map(|p| (p.title.as_str(), p.display_order))
        .collect();
    assert_eq!(
        roots,
        vec![
            ("Getting started", Some(0)),
            ("Network", Some(1)),
            ("Getting started", Some(2)),
            ("Network", Some(3)),
        ]
    );
    assert!(written.iter().all(|p| p.collection_id == Some(collection)));

    // By uuid, into a new collection.
    let existing = written[0].uuid;
    let zip = zip_of(&[
        (
            "start.md",
            format!("---\ntitle: Start again\nuuid: {existing}\n---\n\nHello.\n").as_bytes(),
        ),
        ("other.md", b"# Other\n\nNew.\n"),
    ]);
    let fourth = id_of(&uploaded(&pool, ws, "again.zip", &zip).await);
    let job = checked(&pool, ws, &fourth, serde_json::json!({})).await;
    assert_eq!(job["summary"]["skipped"], 1, "{job}");
    let (_, list, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        pages_request(&fourth, "?filter=skipped"),
        None,
    )
    .await;
    assert_eq!(list["pages"][0]["duplicate"]["by"], "uuid", "{list}");
    assert_eq!(list["pages"][0]["duplicate"]["page_id"], written[0].id);
}

/// A Nosdesk export of three collections, a page in each, and an empty
/// fourth. The page in the second carries `round_trip`.
fn three_collections(ws: &WorkspaceSeed, round_trip: &str) -> Vec<u8> {
    let manifest = format!(
        r#"{{
  "format": "nosdesk-docs",
  "format_version": 2,
  "workspace_uuid": "{workspace}",
  "exported_at": "2026-10-01T00:00:00Z",
  "collections": [
    {{ "uuid": "0b6a6f7e-0001-4000-8000-000000000001", "name": "One", "slug": "one", "path": "one", "order": 1 }},
    {{ "uuid": "0b6a6f7e-0002-4000-8000-000000000002", "name": "Two", "slug": "two", "path": "two", "order": 2 }},
    {{ "uuid": "0b6a6f7e-0003-4000-8000-000000000003", "name": "Three", "slug": "three", "path": "three", "order": 3 }},
    {{ "uuid": "0b6a6f7e-0004-4000-8000-000000000004", "name": "Empty", "slug": "empty", "path": "empty", "order": 4 }}
  ],
  "pages": [
    {{ "uuid": "1c0a0000-0001-4000-8000-000000000001", "title": "First", "slug": "first", "path": "one/first.md", "parent": null, "collection": "0b6a6f7e-0001-4000-8000-000000000001", "order": 1, "status": "published", "icon": null }},
    {{ "uuid": "{round_trip}", "title": "Round trip", "slug": "round-trip", "path": "two/round-trip.md", "parent": null, "collection": "0b6a6f7e-0002-4000-8000-000000000002", "order": 1, "status": "published", "icon": null }},
    {{ "uuid": "1c0a0000-0003-4000-8000-000000000003", "title": "Third", "slug": "third", "path": "three/third.md", "parent": null, "collection": "0b6a6f7e-0003-4000-8000-000000000003", "order": 1, "status": "published", "icon": null }}
  ]
}}"#,
        workspace = ws.workspace_uuid
    );
    let second = format!("---\ntitle: \"Round trip\"\nuuid: {round_trip}\n---\n\nSecond.\n");
    zip_of(&[
        ("nosdesk-docs.json", manifest.as_bytes()),
        (
            "one/first.md",
            b"---\ntitle: \"First\"\nuuid: 1c0a0000-0001-4000-8000-000000000001\n---\n\nFirst.\n",
        ),
        ("two/round-trip.md", second.as_bytes()),
        (
            "three/third.md",
            b"---\ntitle: \"Third\"\nuuid: 1c0a0000-0003-4000-8000-000000000003\n---\n\nThird.\n",
        ),
    ])
}

/// A Nosdesk export imported again as copies with every page but one
/// unticked creates only that page's collection: Review counts the one
/// collection, and the commit writes the one page into it. Imported whole,
/// every collection of the export is created, the empty one included.
#[actix_web::test]
async fn unticked_pages_leave_their_collections_out() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    private_storage();
    let zip = three_collections(ws, "1c0a0000-0002-4000-8000-000000000002");
    let collections = || {
        count(
            &pool,
            ws,
            "SELECT count(*) AS n FROM documentation_collections",
        )
    };
    let before = collections();

    let first = id_of(&uploaded(&pool, ws, "nosdesk-docs.zip", &zip).await);
    let job = checked(&pool, ws, &first, serde_json::json!({})).await;
    assert_eq!(
        job["summary"]["destination"]["new_collections"],
        serde_json::json!(["One", "Two", "Three", "Empty"]),
        "{job}"
    );
    let job = committed(&pool, ws, &first, serde_json::json!({})).await;
    assert_eq!(job["records_committed"], 3, "{job}");
    assert_eq!(collections() - before, 4);

    // Exported again: the page carries the uuid it has here.
    let here = pages(&pool, ws)
        .into_iter()
        .find(|p| p.title == "Round trip")
        .expect("imported")
        .uuid;
    let zip = three_collections(ws, &here.to_string());
    let job = uploaded(&pool, ws, "nosdesk-docs.zip", &zip).await;
    let second = id_of(&job);
    let tree = job["source"]["tree"]["pages"].as_array().unwrap().clone();
    let unticked: Vec<u64> = tree
        .iter()
        .filter(|p| p["title"] != "Round trip")
        .map(|p| p["index"].as_u64().unwrap())
        .collect();
    assert_eq!(unticked.len(), 2, "{tree:?}");
    let job = checked(
        &pool,
        ws,
        &second,
        serde_json::json!({ "exclude_pages": unticked }),
    )
    .await;
    assert_eq!(job["summary"]["duplicates"], 1, "{job}");
    assert_eq!(
        job["summary"]["destination"]["new_collections"],
        serde_json::json!(["Two"]),
        "{job}"
    );
    let job = committed(
        &pool,
        ws,
        &second,
        serde_json::json!({ "duplicates": "copy" }),
    )
    .await;
    assert_eq!(job["status"], "done", "{job}");
    assert_eq!(job["records_committed"], 1, "{job}");
    assert_eq!(collections() - before, 5, "one more collection");
    let copy = pages(&pool, ws)
        .into_iter()
        .filter(|p| p.title == "Round trip")
        .max_by_key(|p| p.id)
        .expect("the copy");
    let named = |id: i32| {
        count(
            &pool,
            ws,
            &format!(
                "SELECT count(*) AS n FROM documentation_collections WHERE id = {id} AND name = 'Two'"
            ),
        )
    };
    assert_eq!(named(copy.collection_id.expect("in a collection")), 1);
}

/// A commit whose transaction fails writes nothing, deletes the images it
/// stored, and goes back on Review saying so.
#[actix_web::test]
async fn a_failed_commit_deletes_the_images_it_stored() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let storage = private_storage();
    let id = id_of(&uploaded(&pool, ws, "handbook.zip", &handbook()).await);
    checked(&pool, ws, &id, serde_json::json!({})).await;

    diesel::sql_query(
        "ALTER TABLE documentation_pages ADD CONSTRAINT test_refuse_wifi CHECK (title <> 'Wi-Fi')",
    )
    .execute(&mut pool.get().expect("conn"))
    .expect("constraint");
    let job = committed(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["status"], "dry_run_done", "{job}");
    assert_eq!(
        job["error_message"],
        "Nothing was imported. The import failed before it finished."
    );
    assert!(job["records_committed"].is_null());
    assert_eq!(page_count(&pool, ws), 0);
    assert_eq!(
        count(
            &pool,
            ws,
            "SELECT count(*) AS n FROM documentation_collections"
        ),
        0
    );
    assert!(
        collab_files(&storage, ws).await.is_empty(),
        "stored images deleted"
    );

    // Fixed, it commits.
    diesel::sql_query("ALTER TABLE documentation_pages DROP CONSTRAINT test_refuse_wifi")
        .execute(&mut pool.get().expect("conn"))
        .expect("drop constraint");
    let job = committed(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["status"], "done", "{job}");
    assert_eq!(page_count(&pool, ws), 4);
    assert_eq!(collab_files(&storage, ws).await.len(), 2);
}

/// A commit whose process died is a clean yes or no: with none of its pages
/// written, its stored images go and the job is back on Review; with them
/// written, it is done.
#[actix_web::test]
async fn a_commit_whose_process_died_is_put_back() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let id = id_of(&uploaded(&pool, ws, "handbook.zip", &handbook()).await);
    checked(&pool, ws, &id, serde_json::json!({})).await;

    // It stored an image and died before its transaction.
    let page = Uuid::now_v7();
    let key = format!("collab/doc/{page}/0199_logo.png");
    // The daily pass reads the process's storage.
    let shared = crate::common::test_storage();
    scoped(&shared, ws)
        .put_file(PNG, &key, "image/png")
        .await
        .expect("put");
    let record = serde_json::json!({
        "kind": "documentation",
        "run": Uuid::now_v7(),
        "pending": true,
        "page_uuids": [page],
        "storage_keys": [key],
    });
    let stall = |record: &serde_json::Value| {
        run_in_workspace(&pool, REF, ws.workspace_id, |c| {
            diesel::sql_query(
                "UPDATE import_jobs SET status = 'committing', record_ids = $2, records_committed = 0, \
                 updated_at = now() - interval '10 minutes' WHERE id = $1",
            )
            .bind::<SqlUuid, _>(Uuid::parse_str(&id).unwrap())
            .bind::<diesel::sql_types::Jsonb, _>(record)
            .execute(c)
        })
        .expect("stall");
    };
    stall(&record);
    // The daily pass puts it back.
    backend::services::scheduled_jobs::reconcile_documentation_imports(&db.job_pool())
        .await
        .expect("reconcile");
    let (_, job, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
        None,
    )
    .await;
    assert_eq!(job["status"], "dry_run_done", "{job}");
    assert_eq!(
        job["error_message"],
        "Nothing was imported. The import stopped before it finished."
    );
    assert!(scoped(&shared, ws).get_file(&key).await.is_err(), "deleted");

    // Still alive (a recent heartbeat): left alone.
    on_job(
        &pool,
        ws,
        "UPDATE import_jobs SET status = 'committing', updated_at = now() WHERE id = $1",
        &id,
    );
    let (_, job, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
        None,
    )
    .await;
    assert_eq!(job["status"], "committing");

    // Its pages exist: it committed.
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "INSERT INTO documentation_pages (uuid, title, slug, status, created_by, last_edited_by) \
             VALUES ($1, 'Written', 'written', 'draft', $2, $2)",
        )
        .bind::<SqlUuid, _>(page)
        .bind::<SqlUuid, _>(ws.admin_uuid)
        .execute(c)
    })
    .expect("page");
    stall(&record);
    let (_, job, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        TestRequest::get().uri(&format!("/api/admin/import/{id}")),
        None,
    )
    .await;
    assert_eq!(job["status"], "done", "{job}");
}

/// Web images are downloaded through the safe client at the check: one on
/// a private address is refused and listed, and the page keeps its alt
/// text. Kept as links, the URL stays the image's source.
#[actix_web::test]
async fn web_images_are_downloaded_or_kept_as_links() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let zip = zip_of(&[(
        "diagram.md",
        b"# Diagram\n\n![rack layout](http://10.0.0.1/rack.png)\n" as &[u8],
    )]);

    let id = id_of(&uploaded(&pool, ws, "web.zip", &zip).await);
    let job = checked(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["summary"]["web_images"], 1, "{job}");
    assert_eq!(job["summary"]["image_problems"], 1, "{job}");
    let (_, list, _) = call_docs(&pool, ws, ws.admin_uuid, pages_request(&id, ""), None).await;
    let image = &list["pages"][0]["images"][0];
    assert_eq!(image["kind"], "web", "{list}");
    assert_eq!(image["problem"], "refused_address", "{list}");
    assert_eq!(
        list["pages"][0]["notes"]["web_images_not_fetched"], 1,
        "{list}"
    );
    committed(&pool, ws, &id, serde_json::json!({})).await;
    let written = pages(&pool, ws);
    let page = by_title(&written, "Diagram");
    assert!(images_of(page).is_empty());
    assert!(body(page).to_string().contains("rack layout"));

    let id = id_of(&uploaded(&pool, ws, "web.zip", &zip).await);
    let job = checked(&pool, ws, &id, serde_json::json!({ "web_images": "links" })).await;
    assert_eq!(job["summary"]["image_problems"], 0, "{job}");
    committed(&pool, ws, &id, serde_json::json!({})).await;
    let written = pages(&pool, ws);
    let kept = written.iter().rev().find(|p| p.title == "Diagram").unwrap();
    assert_eq!(
        images_of(kept),
        vec!["http://10.0.0.1/rack.png".to_string()]
    );
}

/// Audit rows for job `id`.
fn job_audit_rows(pool: &TestPool, ws: &WorkspaceSeed, id: &str) -> i64 {
    count(
        pool,
        ws,
        &format!(
            "SELECT count(*) AS n FROM audit_log WHERE table_name = 'import_jobs' AND pk_text = '{id}'"
        ),
    )
}

/// Unticked pages are left out by their index in the tree Pages showed,
/// with the pages under them; out-of-range indices are refused; the
/// committed job names each page it wrote, by path, with its id.
#[actix_web::test]
async fn unticked_pages_are_left_out_and_the_job_names_the_pages_it_wrote() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    private_storage();
    let job = uploaded(&pool, ws, "IT-Handbook.zip", &handbook()).await;
    let id = id_of(&job);
    let tree = job["source"]["tree"]["pages"].as_array().unwrap().clone();
    let index_of = |title: &str| {
        tree.iter()
            .find(|p| p["title"] == title)
            .unwrap_or_else(|| panic!("no {title} in {tree:?}"))["index"]
            .as_u64()
            .unwrap()
    };
    let network = index_of("Network");
    assert_eq!(tree[network as usize]["kind"], "page");

    let (status, body, _) = call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        crate::imports_docs_flow::options_request(
            &id,
            serde_json::json!({ "exclude_pages": [tree.len()] }),
            true,
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let job = checked(
        &pool,
        ws,
        &id,
        serde_json::json!({ "exclude_pages": [network] }),
    )
    .await;
    assert_eq!(job["options"]["exclude_pages"][0], network);
    assert_eq!(job["summary"]["would_create"], 1, "{job}");
    let job = committed(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["status"], "done", "{job}");
    assert_eq!(job["records_committed"], 1);

    let written = pages(&pool, ws);
    let titles: Vec<&str> = written.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(titles, vec!["Getting started"]);
    let named = job["pages_written"].as_array().expect("pages_written");
    assert_eq!(named.len(), 1, "{job}");
    assert_eq!(named[0]["id"], written[0].id);
    assert_eq!(named[0]["path"], "getting-started.md");
}

/// The commit records its progress after each batch of pages, through the
/// heartbeat, which leaves no audit row: a large import is audited exactly
/// as often as a small one.
#[actix_web::test]
async fn progress_by_batch_leaves_no_audit_rows() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    private_storage();

    let audited_commit = |zip: Vec<u8>, name: &'static str| {
        let pool = pool.clone();
        async move {
            let id = id_of(&uploaded(&pool, ws, name, &zip).await);
            checked(&pool, ws, &id, serde_json::json!({})).await;
            let before = job_audit_rows(&pool, ws, &id);
            let job = committed(&pool, ws, &id, serde_json::json!({})).await;
            assert_eq!(job["status"], "done", "{job}");
            (job, job_audit_rows(&pool, ws, &id) - before)
        }
    };

    let (_, small) = audited_commit(zip_of(&[("one.md", b"# One\n\nOne.")]), "one.zip").await;
    let bodies: Vec<(String, String)> = (0..80)
        .map(|i| {
            (
                format!("page-{i:03}.md"),
                format!("# Page {i}\n\nBody {i}."),
            )
        })
        .collect();
    let files: Vec<(&str, &[u8])> = bodies
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_bytes()))
        .collect();
    let (job, large) = audited_commit(zip_of(&files), "many.zip").await;
    assert_eq!(job["records_committed"], 80);
    assert_eq!(large, small, "batches of progress add no audit rows");
}
