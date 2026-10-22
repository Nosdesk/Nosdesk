//! Hosted: a workspace's own owner and admins download the documentation
//! export, the same as on a self-hosted instance. The route has no Cloud
//! gate, so a Cloud owner isn't left with the API as the only way out.
//! Agents are refused there too.
//!
//! A standalone binary rather than part of `tests/it`: the deployment mode
//! is process-wide environment.

#![allow(clippy::expect_used)]

mod common;

use std::io::{Cursor, Read};

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test::{self as http_test, TestRequest};
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, DocumentationStatus, NewDocumentationPage};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

const REF: &str = "test:hosted_docs_export";
const AS: &str = "x-test-as";

fn claims(user: Uuid) -> Claims {
    let now = chrono::Utc::now().timestamp();
    Claims {
        sub: user.to_string(),
        name: "Hosted Caller".to_string(),
        email: "caller@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    }
}

#[actix_web::test]
async fn hosted_owners_and_admins_download_the_documentation() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    std::env::set_var("NOSDESK_WORKSPACE_SELECTION", "1");
    assert!(backend::middleware::workspace_context::is_hosted());
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    let (workspace_id, owner, admin, agent) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "hosted-docs", "Hosted Docs");
        (
            ws,
            common::insert_plain_user(&mut conn, "Hosted Owner"),
            common::insert_plain_user(&mut conn, "Hosted Admin"),
            common::insert_plain_user(&mut conn, "Hosted Agent"),
        )
    };
    let workspace_uuid: Uuid = backend::schema::workspaces::table
        .find(workspace_id)
        .select(backend::schema::workspaces::uuid)
        .get_result(&mut pool.get().expect("conn"))
        .expect("workspace uuid");
    run_in_workspace(&pool, REF, workspace_id, |c| {
        for (user, role) in [(owner, "owner"), (admin, "admin"), (agent, "agent")] {
            add_membership(
                c,
                workspace_id,
                user,
                role,
                SeatWriteAuthority::ControlPlane,
            )?;
        }
        diesel::insert_into(backend::schema::documentation_pages::table)
            .values(&NewDocumentationPage {
                uuid: Uuid::new_v4(),
                title: "Onboarding".to_string(),
                slug: "onboarding".to_string(),
                icon: None,
                cover_image: None,
                status: DocumentationStatus::Published,
                created_by: owner,
                last_edited_by: owner,
                parent_id: None,
                display_order: None,
                is_public: false,
                is_template: false,
                yjs_state_vector: None,
                yjs_document: None,
                yjs_client_id: None,
                has_unsaved_changes: false,
            })
            .execute(c)?;
        Ok(())
    })
    .expect("seed");

    let storage_dir = tempfile::tempdir().expect("storage dir");
    let storage: std::sync::Arc<dyn Storage> = create_storage(StorageConfig::Local {
        base_path: storage_dir.path().to_string_lossy().into_owned(),
    });
    let workspace = WorkspaceContext {
        workspace_id,
        workspace_uuid,
        slug: "hosted-docs".into(),
        name: "Hosted Docs".into(),
        organisation_id: None,
        custom_domain: None,
    };
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
                let actor = ActorContext::user(user, Some(corr)).with_workspace(workspace_id);
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
        TestRequest::get()
            .uri("/api/documentation/export.zip")
            .insert_header((AS, user.to_string()))
            .to_request()
    };

    for user in [owner, admin] {
        let resp = http_test::call_service(&app, get(user)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let disposition = resp
            .headers()
            .get("content-disposition")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(
            disposition.contains("hosted-docs-documentation-"),
            "{disposition}"
        );
        let bytes = http_test::read_body(resp).await;
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("a zip");
        let mut manifest = String::new();
        archive
            .by_name("nosdesk-docs.json")
            .expect("manifest")
            .read_to_string(&mut manifest)
            .expect("read manifest");
        assert!(manifest.contains("\"Onboarding\""), "{manifest}");
    }

    let resp = http_test::call_service(&app, get(agent)).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
