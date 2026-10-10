//! Instance restore runs as the migration role, so it works where the app
//! connects as `nosdesk_app`, and hosted doesn't offer it.
//!
//! A standalone binary rather than part of `tests/it`: deployment mode and
//! `MIGRATION_DATABASE_URL` are process-wide environment.

#![allow(clippy::expect_used)]

mod common;

use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::middleware::RequestContext;
use backend::models::{BackupJobUpdate, Claims, NewBackupJob};
use backend::repository::backup as backup_repo;
use backend::services::backup as backup_service;
use backend::sync::actor::ActorContext;

use common::TestPool;

/// Call the backup routes as a platform admin in the bootstrap workspace.
async fn as_operator(pool: &TestPool, req: http_test::TestRequest) -> ServiceResponse {
    let operator = Uuid::new_v4();
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: operator.to_string(),
        name: "Operator".to_string(),
        email: "operator@example.com".to_string(),
        platform_role: "platform_admin".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(operator, Some(corr)).with_workspace(1);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::backup::config)),
    )
    .await;
    http_test::call_service(&app, req.to_request()).await
}

#[actix_web::test]
async fn restore_runs_as_the_migration_role_and_hosted_refuses_it() {
    common::ensure_test_keyring();
    common::with_upload_dir();
    std::env::remove_var("NOSDESK_DEPLOYMENT_MODE");
    std::env::remove_var("MIGRATION_DATABASE_URL");
    let db = common::TestDb::new();

    // A backup of this database, and a restore job pointing at it.
    let restore_job = {
        let mut conn = db.conn();
        let export_job = common::seed_backup_job(&mut conn);
        let archive =
            backup_service::create_backup(&mut conn, export_job, None).expect("create backup");
        let job = backup_repo::create_backup_job(
            &mut conn,
            NewBackupJob {
                job_type: "restore".to_string(),
                status: "pending".to_string(),
                include_sensitive: false,
                created_by: None,
            },
        )
        .expect("restore job");
        backup_repo::update_backup_job(
            &mut conn,
            job.id,
            BackupJobUpdate {
                status: None,
                file_path: Some(archive.to_string_lossy().into_owned()),
                file_size: None,
                error_message: None,
                completed_at: None,
            },
        )
        .expect("point the job at the archive");
        job.id
    };
    let pool = db.runtime_pool(4);
    let execute = || {
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/backup/restore/{restore_job}/execute"))
            .set_json(json!({ "password": null }))
    };

    // As the app role alone, the restore can't run.
    assert_eq!(
        as_operator(&pool, execute()).await.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );

    // A migration role that can't be reached is said so, without the
    // connection's details, and the job is marked failed.
    std::env::set_var("MIGRATION_DATABASE_URL", UNREACHABLE_URL);
    std::env::set_var("DB_CONNECTION_TIMEOUT", "1");
    let refused = as_operator(&pool, execute()).await;
    std::env::remove_var("DB_CONNECTION_TIMEOUT");
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = String::from_utf8_lossy(&http_test::read_body(refused).await).into_owned();
    assert!(
        body.contains("MIGRATION_DATABASE_URL is set but couldn't connect"),
        "{body}"
    );
    assert!(!body.contains("127.0.0.1"), "{body}");
    let job = backup_repo::get_backup_job(&mut db.conn(), restore_job).expect("job");
    assert_eq!(job.status, "failed");
    assert_eq!(
        job.error_message.as_deref(),
        Some("database restore failed: MIGRATION_DATABASE_URL is set but couldn't connect")
    );

    // With a migration role configured, it runs as that role.
    std::env::set_var("MIGRATION_DATABASE_URL", db.url());
    let restored = as_operator(&pool, execute()).await;
    assert_eq!(restored.status(), StatusCode::OK);
    let body: serde_json::Value = http_test::read_body_json(restored).await;
    assert!(
        body["tables_restored"].as_u64().is_some_and(|n| n > 0),
        "{body}"
    );

    // A 1.0.12 backup previews as an upgrade and restores, upgraded.
    let old_job = restore_job_for(&db, &common::fixture_path("backups/1.0.12/plain.zip"));
    let preview = as_operator(
        &pool,
        http_test::TestRequest::get().uri(&format!("/api/admin/backup/restore/{old_job}/preview")),
    )
    .await;
    assert_eq!(preview.status(), StatusCode::OK);
    let body: serde_json::Value = http_test::read_body_json(preview).await;
    assert_eq!(body["upgrade"]["from_version"], "1.0.12", "{body}");
    let restored = as_operator(
        &pool,
        http_test::TestRequest::post()
            .uri(&format!("/api/admin/backup/restore/{old_job}/execute"))
            .set_json(json!({ "password": null })),
    )
    .await;
    assert_eq!(restored.status(), StatusCode::OK);
    let merges: i64 =
        diesel::dsl::sql::<diesel::sql_types::BigInt>("SELECT count(*) FROM ticket_merges")
            .get_result(&mut db.conn())
            .expect("ticket_merges");
    assert_eq!(merges, 1, "the 1.0 merge survives the upgrade");

    // Hosted doesn't offer it.
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    assert_eq!(
        as_operator(&pool, execute()).await.status(),
        StatusCode::FORBIDDEN
    );
    let preview = http_test::TestRequest::get()
        .uri(&format!("/api/admin/backup/restore/{restore_job}/preview"));
    assert_eq!(
        as_operator(&pool, preview).await.status(),
        StatusCode::FORBIDDEN
    );
}

/// A restore job pointing at `archive`.
fn restore_job_for(db: &common::TestDb, archive: &std::path::Path) -> Uuid {
    let mut conn = db.conn();
    let job = backup_repo::create_backup_job(
        &mut conn,
        NewBackupJob {
            job_type: "restore".to_string(),
            status: "pending".to_string(),
            include_sensitive: false,
            created_by: None,
        },
    )
    .expect("restore job");
    backup_repo::update_backup_job(
        &mut conn,
        job.id,
        BackupJobUpdate {
            status: None,
            file_path: Some(archive.to_string_lossy().into_owned()),
            file_size: None,
            error_message: None,
            completed_at: None,
        },
    )
    .expect("point the job at the archive");
    job.id
}

/// Nothing listens on the discard port, so a connection here is refused.
const UNREACHABLE_URL: &str = "postgres://nosdesk:nosdesk_password@127.0.0.1:9/nosdesk";

/// `nosdesk-cli db restore` is the recovery path for when the app can't
/// serve. Where the app connects as `nosdesk_app`, the CLI restores as the
/// migration role, like the admin restore.
#[test]
fn the_cli_restores_as_the_migration_role() {
    common::ensure_test_keyring();
    common::with_upload_dir();
    let db = common::TestDb::new();
    let archive = {
        let mut conn = db.conn();
        let export_job = common::seed_backup_job(&mut conn);
        backup_service::create_backup(&mut conn, export_job, None).expect("create backup")
    };
    let sep = if db.url().contains('?') { '&' } else { '?' };
    let app_url = format!("{}{sep}options=-c%20role%3Dnosdesk_app", db.url());

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_nosdesk-cli"))
        .args(["db", "restore", "--yes", "--force"])
        .arg(&archive)
        .env("DATABASE_URL", &app_url)
        .env("MIGRATION_DATABASE_URL", db.url())
        .env(
            "UPLOAD_DIR",
            std::env::var("UPLOAD_DIR").expect("UPLOAD_DIR"),
        )
        .env_remove("NOSDESK_DEPLOYMENT_MODE")
        .output()
        .expect("run nosdesk-cli");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "nosdesk-cli db restore failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("Restore complete"), "{stdout}");
}

/// A migration role that can't be reached stops the CLI restore with that
/// reason, instead of a permission error from running as the app role.
#[test]
fn the_cli_says_when_the_migration_role_is_unreachable() {
    common::ensure_test_keyring();
    common::with_upload_dir();
    let db = common::TestDb::new();
    let archive = {
        let mut conn = db.conn();
        let export_job = common::seed_backup_job(&mut conn);
        backup_service::create_backup(&mut conn, export_job, None).expect("create backup")
    };
    let sep = if db.url().contains('?') { '&' } else { '?' };
    let app_url = format!("{}{sep}options=-c%20role%3Dnosdesk_app", db.url());

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_nosdesk-cli"))
        .args(["db", "restore", "--yes", "--force"])
        .arg(&archive)
        .env("DATABASE_URL", &app_url)
        .env("MIGRATION_DATABASE_URL", UNREACHABLE_URL)
        .env("DB_CONNECTION_TIMEOUT", "1")
        .env(
            "UPLOAD_DIR",
            std::env::var("UPLOAD_DIR").expect("UPLOAD_DIR"),
        )
        .env_remove("NOSDESK_DEPLOYMENT_MODE")
        .output()
        .expect("run nosdesk-cli");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("MIGRATION_DATABASE_URL is set but couldn't connect"),
        "{stderr}"
    );
    assert!(!stderr.contains("permission denied"), "{stderr}");
}
