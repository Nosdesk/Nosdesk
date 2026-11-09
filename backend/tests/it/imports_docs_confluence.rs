//! A Confluence Data Center HTML export through the documentation import:
//! uploaded as Confluence writes the zip, checked, committed with its
//! links pointing at the pages it wrote, and found again when imported a
//! second time into the same collection.

use std::io::Write as _;
use std::path::Path;

use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::sync::session::run_in_workspace;

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::import_workspaces;
use crate::imports_docs_flow::{checked, committed, id_of, page_count, private_storage, uploaded};

const REF: &str = "test:imports_docs_confluence";

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/docs_import/confluence-dc"
);

/// The fixture export zipped as Confluence zips it: a `/` folder entry
/// first, then each folder and file.
fn export_zip() -> Vec<u8> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).expect("fixture dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, base, out);
            } else {
                let key = path
                    .strip_prefix(base)
                    .expect("under base")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((key, std::fs::read(&path).expect("reads")));
            }
        }
    }
    let base = Path::new(FIXTURE);
    let mut files = Vec::new();
    walk(base, base, &mut files);
    files.sort();
    let options = zip::write::SimpleFileOptions::default();
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.add_directory("/", options).expect("root folder");
    let mut folders: Vec<String> = files
        .iter()
        .flat_map(|(k, _)| {
            let parts: Vec<&str> = k.split('/').collect();
            (1..parts.len())
                .map(|n| parts[..n].join("/"))
                .collect::<Vec<_>>()
        })
        .collect();
    folders.sort();
    folders.dedup();
    for folder in folders {
        w.add_directory(format!("{folder}/"), options)
            .expect("folder");
    }
    for (key, bytes) in &files {
        w.start_file(key.as_str(), options).expect("start file");
        w.write_all(bytes).expect("write file");
    }
    w.finish().expect("finish zip").into_inner()
}

#[derive(QueryableByName, Debug)]
struct PageRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = SqlUuid)]
    uuid: Uuid,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Nullable<Integer>)]
    parent_id: Option<i32>,
    #[diesel(sql_type = Nullable<Integer>)]
    display_order: Option<i32>,
    #[diesel(sql_type = Nullable<Integer>)]
    collection_id: Option<i32>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Bytea>)]
    yjs_document: Option<Vec<u8>>,
}

fn pages(pool: &TestPool, ws: &WorkspaceSeed) -> Vec<PageRow> {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "SELECT p.id, p.uuid, p.title, p.parent_id, p.display_order, cp.collection_id, \
             p.yjs_document FROM documentation_pages p \
             LEFT JOIN documentation_collection_pages cp ON cp.page_id = p.id ORDER BY p.id",
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

/// Every value of `key` anywhere in a page's ProseMirror JSON.
fn values_of(page: &PageRow, key: &str) -> Vec<String> {
    fn walk(value: &serde_json::Value, key: &str, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    if k == key {
                        if let Some(s) = v.as_str() {
                            out.push(s.to_string());
                        }
                    }
                    walk(v, key, out);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, key, out)),
            _ => {}
        }
    }
    let json = backend::services::docs_markdown::encode::decode::to_json(
        page.yjs_document.as_deref().expect("a body"),
    );
    let mut out = Vec::new();
    walk(&json, key, &mut out);
    out
}

#[actix_web::test]
async fn a_confluence_export_imports_with_its_tree_links_and_images() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let _storage = private_storage();
    let zip = export_zip();

    let job = uploaded(&pool, ws, "Confluence-space-export-021440-2.html.zip", &zip).await;
    let id = id_of(&job);
    let tree = &job["source"]["tree"];
    assert_eq!(tree["profile"], "confluence", "{tree}");
    assert_eq!(
        tree["pages"][0]["title"], "QA Knowledge Base Home",
        "{tree}"
    );
    let job = checked(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["summary"]["would_create"], 17, "{job}");
    let job = committed(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["status"], "done", "{job}");
    assert_eq!(job["records_committed"], 17, "{job}");

    let written = pages(&pool, ws);
    assert_eq!(written.len(), 17);
    let home = by_title(&written, "QA Knowledge Base Home");
    let links = by_title(&written, "Links");
    let getting = by_title(&written, "Getting Started");
    let install = by_title(&written, "Install Guide");
    let faq = by_title(&written, "FAQ");
    let special = by_title(&written, "Q&A: What's new? / \u{dc}n\u{ef}code \u{2713}");
    let media = by_title(&written, "Images and Attachments");

    // One collection named after the space; the homepage first with its
    // children beside it.
    assert!(written
        .iter()
        .all(|p| p.collection_id.is_some() && p.collection_id == home.collection_id));
    #[derive(QueryableByName)]
    struct Name {
        #[diesel(sql_type = Text)]
        name: String,
    }
    let collection = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("SELECT name FROM documentation_collections WHERE id = $1")
            .bind::<Integer, _>(home.collection_id.unwrap())
            .get_result::<Name>(c)
    })
    .expect("collection");
    assert_eq!(collection.name, "QA Knowledge Base");
    assert_eq!((home.parent_id, home.display_order), (None, Some(0)));
    assert_eq!((links.parent_id, links.display_order), (None, Some(4)));
    assert_eq!((getting.parent_id, getting.display_order), (None, Some(5)));
    assert_eq!(install.parent_id, Some(getting.id));
    assert_eq!(
        by_title(&written, "Deep Nested Page").parent_id,
        Some(install.id)
    );

    // Relative, viewpage.action and /display/ links reach the pages.
    let to = |p: &PageRow| format!("/documentation/{}", p.id);
    let hrefs = values_of(links, "href");
    assert_eq!(
        hrefs,
        [
            to(install),
            to(install),
            to(install),
            to(getting),
            to(install),
            to(faq),
            "https://example.atlassian.net/wiki/spaces/QA/pages/294922/Install+Guide".into(),
            "https://confluence.example.com/spaces/OPS/pages/294918/Ops+Runbook".into(),
            "https://confluence.example.com/pages/viewpage.action?pageId=294918".into(),
            "https://example.com/docs".into(),
            "mailto:help@example.com".into(),
            to(special),
        ]
    );

    // The attachments shown as images are stored under the page.
    let prefix = format!("/api/files/collab/doc/{}/", media.uuid);
    let stored: Vec<String> = values_of(media, "src")
        .into_iter()
        .filter(|s| s.starts_with(&prefix))
        .collect();
    assert_eq!(stored.len(), 4, "{:?}", values_of(media, "src"));

    // The same export into the collection it made finds every page.
    let again =
        id_of(&uploaded(&pool, ws, "Confluence-space-export-021440-2.html.zip", &zip).await);
    let job = checked(
        &pool,
        ws,
        &again,
        serde_json::json!({ "collections": "existing", "collection_id": home.collection_id }),
    )
    .await;
    assert_eq!(job["summary"]["duplicates"], 17, "{job}");
    assert_eq!(job["summary"]["would_create"], 0, "{job}");
    let job = committed(&pool, ws, &again, serde_json::json!({})).await;
    assert_eq!(job["records_committed"], 0, "{job}");
    assert_eq!(page_count(&pool, ws), 17);
}

#[actix_web::test]
async fn an_xml_space_backup_is_refused_at_upload() {
    let db = TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    let _storage = private_storage();
    let base = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/docs_import/confluence-dc-xml"
    ));
    let zip = crate::imports_docs_flow::zip_of(&[
        (
            "entities.xml",
            &std::fs::read(base.join("entities.xml")).expect("entities"),
        ),
        (
            "exportDescriptor.properties",
            &std::fs::read(base.join("exportDescriptor.properties")).expect("descriptor"),
        ),
    ]);
    let (status, body, _) = crate::imports_docs_flow::call_docs(
        &pool,
        ws,
        ws.admin_uuid,
        crate::imports_docs_flow::upload_files(&[("Confluence-space-export-QA.xml.zip", &zip)]),
        None,
    )
    .await;
    assert_eq!(status, actix_web::http::StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["code"], "docs_import_confluence_xml", "{body}");
}
