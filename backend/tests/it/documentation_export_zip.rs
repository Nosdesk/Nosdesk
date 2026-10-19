//! `GET /api/documentation/export.zip`: staff only, the same pages the JSON
//! export lists for the caller, every page a file (two siblings with one
//! title both), the manifest naming no page or collection the caller can't
//! open, and the images a page shows copied into `_assets/`.

#![allow(clippy::expect_used)]

use std::io::{Cursor, Read};
use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::Value;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{
    Claims, DocumentationStatus, NewDocumentationCollection, NewDocumentationCollectionPage,
    NewDocumentationPage,
};
use backend::repository::documentation_collections;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::services::docs_markdown::encode;
use backend::services::docs_markdown::ir::{Block, Document, Image, Inline, Link, Marks};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig, WorkspaceScopedStorage};

use crate::common::{self, TestPool};

const REF: &str = "test:documentation_export_zip";

struct NewPage<'a> {
    uuid: Uuid,
    title: &'a str,
    parent: Option<i32>,
    order: Option<i32>,
    body: Option<Vec<u8>>,
}

fn page(conn: &mut backend::db::DbConnection, author: Uuid, p: NewPage<'_>) -> (i32, Uuid, String) {
    use backend::schema::documentation_pages;
    let uuid = p.uuid;
    let slug = format!(
        "{}-{}",
        p.title.to_lowercase().replace(' ', "-"),
        &Uuid::new_v4().simple().to_string()[..6]
    );
    let id = diesel::insert_into(documentation_pages::table)
        .values(&NewDocumentationPage {
            uuid,
            title: p.title.to_string(),
            slug: slug.clone(),
            icon: None,
            cover_image: None,
            status: DocumentationStatus::Published,
            created_by: author,
            last_edited_by: author,
            parent_id: p.parent,
            display_order: p.order,
            is_public: false,
            is_template: false,
            yjs_state_vector: None,
            yjs_document: p.body,
            yjs_client_id: None,
            has_unsaved_changes: false,
        })
        .returning(documentation_pages::id)
        .get_result(conn)
        .expect("insert page");
    (id, uuid, slug)
}

fn collection(conn: &mut backend::db::DbConnection, name: &str) -> i32 {
    documentation_collections::create_collection(
        conn,
        NewDocumentationCollection {
            uuid: Uuid::now_v7(),
            name: name.to_string(),
            slug: format!(
                "{}-{}",
                name.to_lowercase(),
                &Uuid::new_v4().simple().to_string()[..6]
            ),
            description: Some(format!("{name} pages")),
            icon: None,
            color: None,
            is_system: false,
            created_by: None,
        },
    )
    .expect("create collection")
    .id
}

fn add_to(conn: &mut backend::db::DbConnection, collection_id: i32, page_id: i32) {
    documentation_collections::add_page_to_collection(
        conn,
        NewDocumentationCollectionPage {
            collection_id,
            page_id,
            created_by: None,
        },
    )
    .expect("add to collection");
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

const AS: &str = "x-test-as";

struct Fixture {
    workspace: WorkspaceContext,
    admin: Uuid,
    member: Uuid,
    outsider: Uuid,
    guide_uuid: Uuid,
    hidden_title: &'static str,
    secret_page_title: &'static str,
}

fn setup(pool: &TestPool, storage_root: &std::path::Path) -> (Fixture, Arc<dyn Storage>) {
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let (insider, outsider) = {
        let mut conn = pool.get().expect("conn");
        (
            common::insert_plain_user(&mut conn, "Insider Agent"),
            common::insert_plain_user(&mut conn, "Outsider Agent"),
        )
    };
    let author = seeded.a.admin_uuid;
    let guide_uuid = Uuid::new_v4();

    run_in_workspace(pool, REF, ws, |c| {
        for agent in [insider, outsider] {
            add_membership(c, ws, agent, "agent", SeatWriteAuthority::ControlPlane)?;
        }
        let guides = collection(c, "Guides");
        let vpn = page(
            c,
            author,
            NewPage {
                uuid: Uuid::new_v4(),
                title: "VPN",
                parent: None,
                order: Some(0),
                body: None,
            },
        );
        add_to(c, guides, vpn.0);
        // Two children with one title.
        for order in [0, 1] {
            let setup = page(
                c,
                author,
                NewPage {
                    uuid: Uuid::new_v4(),
                    title: "Setup",
                    parent: Some(vpn.0),
                    order: Some(order),
                    body: None,
                },
            );
            add_to(c, guides, setup.0);
        }

        // A page that links the VPN page and shows an image of its own.
        let body = encode(&Document {
            blocks: vec![Block::Paragraph(vec![
                Inline::marked("Read", Marks::strong()),
                Inline::text(" the "),
                Inline::marked(
                    "VPN page",
                    Marks {
                        link: Some(Link {
                            href: format!("/documentation/{}", vpn.2),
                            title: None,
                        }),
                        ..Marks::default()
                    },
                ),
                Inline::Image(Image {
                    src: format!("/api/files/collab/doc/{guide_uuid}/0190_map.png"),
                    alt: Some("Map".to_string()),
                    ..Image::default()
                }),
            ])],
        });
        let guide = page(
            c,
            author,
            NewPage {
                uuid: guide_uuid,
                title: "Guide",
                parent: None,
                order: Some(1),
                body: Some(body),
            },
        );
        add_to(c, guides, guide.0);

        let hidden = page(
            c,
            author,
            NewPage {
                uuid: Uuid::new_v4(),
                title: "Hidden Quokka",
                parent: None,
                order: None,
                body: None,
            },
        );
        backend::repository::set_page_visibility(c, hidden.0, vec![], vec![insider], None)?;

        let secret = collection(c, "Secret");
        documentation_collections::set_collection_visibility(
            c,
            secret,
            vec![],
            vec![insider],
            None,
        )?;
        let in_secret = page(
            c,
            author,
            NewPage {
                uuid: Uuid::new_v4(),
                title: "Secret Wombat",
                parent: None,
                order: None,
                body: None,
            },
        );
        add_to(c, secret, in_secret.0);
        Ok(())
    })
    .expect("seed docs");

    let storage: Arc<dyn Storage> = create_storage(StorageConfig::Local {
        base_path: storage_root.to_string_lossy().into_owned(),
    });
    (
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
            outsider,
            guide_uuid,
            hidden_title: "Hidden Quokka",
            secret_page_title: "Secret Wombat",
        },
        storage,
    )
}

/// Every entry of a zip, by name.
fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("a zip");
    (0..archive.len())
        .map(|i| {
            let mut file = archive.by_index(i).expect("entry");
            let mut data = Vec::new();
            file.read_to_end(&mut data).expect("read entry");
            (file.name().expect("entry name").to_string(), data)
        })
        .collect()
}

#[actix_web::test]
async fn the_docs_zip_holds_what_the_caller_may_read() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let storage_dir = tempfile::tempdir().expect("storage dir");
    let (f, storage) = setup(&pool, storage_dir.path());
    WorkspaceScopedStorage::arc(storage.clone(), f.workspace.workspace_id)
        .put_file(
            b"PNG",
            &format!("collab/doc/{}/0190_map.png", f.guide_uuid),
            "image/png",
        )
        .await
        .expect("store image");

    let workspace = f.workspace.clone();
    let corr = Uuid::now_v7();
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(storage))
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
    let get = |user: Uuid| {
        http_test::TestRequest::get()
            .uri("/api/documentation/export.zip")
            .insert_header((AS, user.to_string()))
            .to_request()
    };

    // A requester gets nothing, as from the JSON export.
    let resp = http_test::call_service(&app, get(f.member)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let resp = http_test::call_service(&app, get(f.outsider)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/zip")
    );
    let bytes = http_test::read_body(resp).await;
    let files = entries(&bytes);
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    let text = |name: &str| -> String {
        let (_, data) = files
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} in {names:?}"));
        String::from_utf8(data.clone()).expect("utf-8")
    };

    let manifest: Value = serde_json::from_str(&text("nosdesk-docs.json")).expect("manifest");
    assert_eq!(manifest["format"], "nosdesk-docs");
    assert_eq!(manifest["format_version"], 2);
    assert_eq!(
        manifest["workspace_uuid"],
        f.workspace.workspace_uuid.to_string()
    );
    let titles: Vec<&str> = manifest["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .filter_map(|p| p["title"].as_str())
        .collect();
    assert_eq!(
        titles.iter().filter(|t| **t == "Setup").count(),
        2,
        "both same-title siblings: {titles:?}"
    );
    for absent in [f.hidden_title, f.secret_page_title] {
        assert!(!titles.contains(&absent), "{absent} in {titles:?}");
        let slug = absent.to_lowercase().replace(' ', "-");
        assert!(
            !names.iter().any(|n| n.contains(&slug)),
            "{absent} has no file: {names:?}"
        );
    }
    let collections: Vec<&str> = manifest["collections"]
        .as_array()
        .expect("collections")
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect();
    assert!(collections.contains(&"Guides"), "{collections:?}");
    assert!(!collections.contains(&"Secret"), "{collections:?}");

    // Every manifest path is a file, and each is unique.
    let paths: Vec<&str> = manifest["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .filter_map(|p| p["path"].as_str())
        .collect();
    for path in &paths {
        assert!(names.contains(path), "{path} in {names:?}");
    }
    let unique: std::collections::HashSet<&&str> = paths.iter().collect();
    assert_eq!(unique.len(), paths.len(), "{paths:?}");

    // The children sit in the parent's folder, under its collection.
    let guide_entry = manifest["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .find(|p| p["title"] == "Guide")
        .expect("guide in manifest");
    let guide_path = guide_entry["path"].as_str().expect("path");
    let vpn_path = manifest["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .find(|p| p["title"] == "VPN")
        .and_then(|p| p["path"].as_str())
        .expect("vpn path");
    let vpn_file = vpn_path.rsplit('/').next().expect("file name");
    let vpn_folder = format!("{}/", vpn_path.trim_end_matches(".md"));
    let setups: Vec<&&str> = paths
        .iter()
        .filter(|p| p.starts_with(&vpn_folder))
        .collect();
    assert_eq!(setups.len(), 2, "{paths:?}");

    // The body is Markdown, with the link made relative and the image copied.
    let guide = text(guide_path);
    assert!(guide.starts_with("---\ntitle: \"Guide\"\n"), "{guide}");
    assert!(
        guide.contains(&format!(
            "**Read** the [VPN page]({vpn_file})![Map](../_assets/{}/0190_map.png)",
            f.guide_uuid
        )),
        "{guide}"
    );
    let image = files
        .iter()
        .find(|(n, _)| *n == format!("_assets/{}/0190_map.png", f.guide_uuid))
        .expect("image copied");
    assert_eq!(image.1, b"PNG");

    // An admin sees the restricted pages too.
    let resp = http_test::call_service(&app, get(f.admin)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = http_test::read_body(resp).await;
    let manifest: Value = serde_json::from_str(
        &String::from_utf8(
            entries(&bytes)
                .into_iter()
                .find(|(n, _)| n == "nosdesk-docs.json")
                .expect("manifest")
                .1,
        )
        .expect("utf-8"),
    )
    .expect("manifest");
    let titles: Vec<&str> = manifest["pages"]
        .as_array()
        .expect("pages")
        .iter()
        .filter_map(|p| p["title"].as_str())
        .collect();
    assert!(titles.contains(&f.hidden_title), "{titles:?}");
    assert!(titles.contains(&f.secret_page_title), "{titles:?}");
}
