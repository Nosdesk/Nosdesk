//! A BookStack Portable ZIP through the documentation import: the check
//! reads it as a book, the commit writes its tree with links to pages,
//! chapters and the book's collection filled in and its images stored, and
//! the same export imported again into that collection is skipped. A zip of
//! exports imports a collection each.

use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text};

use backend::sync::session::run_in_workspace;

use crate::common::{TestDb, TestPool, WorkspaceSeed};
use crate::imports_assets::import_workspaces;
use crate::imports_docs_flow::{
    call_docs, checked, committed, id_of, pages_request, private_storage, uploaded, zip_of,
};

const REF: &str = "test:imports_docs_bookstack";
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/docs_import");

/// A fixture directory (`data.json` and `files/`) as a zip.
fn export(name: &str) -> Vec<u8> {
    let dir = std::path::Path::new(FIXTURES).join(name);
    let mut entries: Vec<(String, Vec<u8>)> = vec![(
        "data.json".into(),
        std::fs::read(dir.join("data.json")).expect("data.json"),
    )];
    let mut files: Vec<_> = std::fs::read_dir(dir.join("files"))
        .expect("files")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    files.sort();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        entries.push((format!("files/{name}"), std::fs::read(&path).expect("file")));
    }
    let refs: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    zip_of(&refs)
}

#[derive(QueryableByName, Debug)]
struct PageRow {
    #[diesel(sql_type = Integer)]
    id: i32,
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
            "SELECT p.id, p.title, p.parent_id, p.display_order, cp.collection_id, p.yjs_document \
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

/// Every value of `key` anywhere in a page's body, as strings.
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
    let body = backend::services::docs_markdown::encode::decode::to_json(
        page.yjs_document.as_deref().expect("a body"),
    );
    let mut out = Vec::new();
    walk(&body, key, &mut out);
    out
}

#[derive(QueryableByName)]
struct Collection {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Text)]
    name: String,
    #[diesel(sql_type = Text)]
    slug: String,
    #[diesel(sql_type = Nullable<Text>)]
    description: Option<String>,
}

fn collections(pool: &TestPool, ws: &WorkspaceSeed) -> Vec<Collection> {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "SELECT id, name, slug, description FROM documentation_collections ORDER BY id",
        )
        .load::<Collection>(c)
    })
    .expect("collections")
}

#[actix_web::test]
async fn a_bookstack_book_imports_its_tree_links_and_images() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    private_storage();
    let book = export("bookstack-book");

    let id = id_of(&uploaded(&pool, ws, "Help Desk Handbook.zip", &book).await);
    let job = checked(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["summary"]["profile"], "bookstack", "{job}");
    assert_eq!(job["summary"]["would_create"], 6, "{job}");

    // Review lists each page by its BookStack key, and the attachments the
    // import leaves out by name.
    let (_, list, _) = call_docs(&pool, ws, ws.admin_uuid, pages_request(&id, ""), None).await;
    let rows = list["pages"].as_array().expect("rows");
    let vpn_row = rows
        .iter()
        .find(|r| r["title"] == "VPN setup")
        .expect("vpn row");
    assert_eq!(
        vpn_row["path"],
        "bookstack:00000000-0000-4000-8000-000000000001:page:19"
    );
    assert_eq!(
        vpn_row["links"]["attachments"],
        serde_json::json!(["VPN profile guide.pdf"])
    );
    // Where it sits reads as its titles, and its BookStack id goes to the
    // report.
    assert_eq!(vpn_row["display"], "Onboarding / VPN setup");
    assert_eq!(vpn_row["source_id"], "page:19");
    assert_eq!(vpn_row["no_content"], false);

    let job = committed(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["status"], "done", "{job}");
    assert_eq!(job["records_committed"], 6);

    // One collection named after the book, its description as text.
    let created = collections(&pool, ws);
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].name, "Help Desk Handbook");
    assert_eq!(
        created[0].description.as_deref(),
        Some("How the help desk works. See Welcome first.")
    );

    // The tree in BookStack's order, chapters as pages with theirs under.
    let written = pages(&pool, ws);
    let welcome = by_title(&written, "Welcome");
    let onboarding = by_title(&written, "Onboarding");
    let vpn = by_title(&written, "VPN setup");
    let printer = by_title(&written, "Printer troubleshooting");
    let faq = by_title(&written, "FAQ");
    let top: Vec<(&str, Option<i32>)> = written
        .iter()
        .filter(|p| p.parent_id.is_none())
        .map(|p| (p.title.as_str(), p.display_order))
        .collect();
    assert_eq!(
        top,
        [
            ("Welcome", Some(0)),
            ("Onboarding", Some(1)),
            ("FAQ", Some(2)),
            ("Empty chapter", Some(3)),
        ]
    );
    assert_eq!(vpn.parent_id, Some(onboarding.id));
    assert_eq!(printer.parent_id, Some(onboarding.id));
    assert_eq!(
        (vpn.display_order, printer.display_order),
        (Some(0), Some(1))
    );
    assert!(written
        .iter()
        .filter(|p| p.parent_id.is_none())
        .all(|p| p.collection_id == Some(created[0].id)));

    // Links: pages and chapters by id, the book to its collection, the rest
    // of the web as written.
    assert_eq!(
        values_of(welcome, "href"),
        [
            format!("/documentation/{}", vpn.id),
            format!("/documentation/{}", onboarding.id),
            format!("/documentation/collections/{}", created[0].slug),
            "https://docs.example.com/books/other-book/page/elsewhere".to_string(),
            "https://www.bookstackapp.com/docs/".to_string(),
            "mailto:help@example.com".to_string(),
        ]
    );
    assert_eq!(
        values_of(faq, "href"),
        [
            format!("/documentation/{}", welcome.id),
            format!("/documentation/{}", printer.id),
        ]
    );
    // Images stored under the page that shows them, the shared one too.
    for (page, count) in [(welcome, 1), (vpn, 1), (printer, 2)] {
        let srcs = values_of(page, "src");
        assert_eq!(srcs.len(), count, "{}: {srcs:?}", page.title);
        let prefix = "/api/files/collab/doc/";
        assert!(srcs.iter().all(|s| s.starts_with(prefix)), "{srcs:?}");
    }

    // The same export again, into that collection: every page is skipped.
    let again = id_of(&uploaded(&pool, ws, "Help Desk Handbook.zip", &book).await);
    let job = checked(
        &pool,
        ws,
        &again,
        serde_json::json!({ "collections": "existing", "collection_id": created[0].id }),
    )
    .await;
    assert_eq!(job["summary"]["duplicates"], 6, "{job}");
    assert_eq!(job["summary"]["would_create"], 0, "{job}");
}

#[actix_web::test]
async fn a_zip_of_bookstack_exports_imports_a_collection_each() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = import_workspaces(&pool);
    let ws = &seeded.a;
    private_storage();
    let bundle = zip_of(&[
        ("Help Desk Handbook.zip", &export("bookstack-book")),
        ("VPN setup.zip", &export("bookstack-page")),
    ]);
    let id = id_of(&uploaded(&pool, ws, "books.zip", &bundle).await);
    let job = checked(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["summary"]["profile"], "bookstack", "{job}");
    let job = committed(&pool, ws, &id, serde_json::json!({})).await;
    assert_eq!(job["status"], "done", "{job}");
    assert_eq!(job["records_committed"], 7);
    let created = collections(&pool, ws);
    let names: Vec<&str> = created.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Help Desk Handbook", "VPN setup"]);
    // The page export's own copy links where its export pointed: the web.
    let written = pages(&pool, ws);
    let copy = written
        .iter()
        .find(|p| p.collection_id == Some(created[1].id))
        .expect("copy");
    assert_eq!(copy.title, "VPN setup");
    let hrefs = values_of(copy, "href");
    assert!(
        hrefs.contains(
            &"https://docs.example.com/books/help-desk-handbook/page/welcome".to_string()
        ),
        "{hrefs:?}"
    );
}
