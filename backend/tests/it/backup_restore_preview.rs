//! The restore preview of an encrypted backup. Without the password it says
//! the backup is encrypted and needs one (a normal answer, not a server
//! error); with the right password it shows what the backup holds; a wrong
//! password is the caller's mistake.

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::services::backup as backup_service;
use backend::sync::actor::ActorContext;

use crate::common::{insert_user, seed_backup_job, with_upload_dir, TestDb};

#[actix_web::test]
async fn an_encrypted_backup_previews_once_its_password_is_given() {
    let db = TestDb::new();
    let mut conn = db.conn();
    with_upload_dir();
    insert_user(&mut conn, "Preview Pat");
    let job = seed_backup_job(&mut conn);
    backup_service::create_backup(&mut conn, job, Some("open-sesame")).expect("encrypted backup");

    let (workspace_id, workspace_uuid, slug): (i32, Uuid, String) = {
        use backend::schema::{backup_jobs, workspaces};
        let id: i32 = backup_jobs::table
            .find(job)
            .select(backup_jobs::workspace_id)
            .first(&mut conn)
            .expect("job workspace");
        workspaces::table
            .find(id)
            .select((workspaces::id, workspaces::uuid, workspaces::slug))
            .first(&mut conn)
            .expect("workspace")
    };
    drop(conn);
    let admin = Uuid::now_v7();
    let workspace = WorkspaceContext {
        workspace_id,
        workspace_uuid,
        slug,
        name: "Default".to_string(),
        organisation_id: None,
        custom_domain: None,
    };
    let claims = Claims {
        sub: admin.to_string(),
        name: "Admin".to_string(),
        email: "admin@example.com".to_string(),
        platform_role: "platform_admin".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(admin, Some(corr)).with_workspace(workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(db.pool()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::backup::config)),
    )
    .await;
    let uri = format!("/api/admin/backup/restore/{job}/preview");

    // No password yet: encrypted, and a password is needed.
    let resp =
        http_test::call_service(&app, http_test::TestRequest::get().uri(&uri).to_request()).await;
    assert_eq!(resp.status().as_u16(), 200, "preview without the password");
    let body: Value = http_test::read_body_json(resp).await;
    assert_eq!(body["encrypted"], json!(true), "{body}");
    assert_eq!(body["password_required"], json!(true), "{body}");

    // The right password opens it.
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri(&uri)
            .set_json(json!({ "password": "open-sesame" }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 200, "preview with the password");
    let body: Value = http_test::read_body_json(resp).await;
    assert_eq!(body["password_required"], json!(false), "{body}");
    assert!(body["manifest"]["tables"].is_object(), "{body}");

    // A wrong one is the caller's mistake.
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri(&uri)
            .set_json(json!({ "password": "wrong" }))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 400, "preview with a wrong password");
    let body: Value = http_test::read_body_json(resp).await;
    assert_eq!(body["code"], json!("BACKUP_WRONG_PASSWORD"), "{body}");
}
