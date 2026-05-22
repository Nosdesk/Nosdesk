//! Admin bulk-import endpoints.
//!
//! - `POST /admin/import?type=assets` accepts a multipart CSV
//!   upload, parses + validates, writes an `import_jobs` row
//!   with status `dry_run_done` and returns the summary the
//!   admin reviews before committing.
//! - `POST /admin/import/{id}/commit` applies the validated
//!   rows in a single transaction and flips status to `done`.
//! - `GET  /admin/import/{id}` returns the job row + summary so
//!   the admin can resume.
//! - `GET  /admin/import/template/{type}` returns a CSV header
//!   row the admin can fill in.

use actix_multipart::Multipart;
use actix_web::{http::header, web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::prelude::*;
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::{errors, helpers};
use crate::models::{ImportJobUpdate, NewImportJob};
use crate::repository::imports as repo;
use crate::services::imports::{self, csv_parser, ImportType};
use crate::utils;

#[derive(Debug, Deserialize)]
pub struct UploadQuery {
    #[serde(rename = "type")]
    pub job_type: String,
}

/// Upload + parse + dry-run.
pub async fn upload(
    req: HttpRequest,
    pool: web::Data<Pool>,
    query: web::Query<UploadQuery>,
    mut payload: Multipart,
) -> impl Responder {
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(c) => c.clone(),
        None => return errors::unauthorized("Authentication required"),
    };
    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(u) => u,
        Err(_) => return errors::bad_request("invalid user UUID"),
    };

    let job_type = match ImportType::from_str(query.job_type.as_str()) {
        Some(t) => t,
        None => {
            return errors::bad_request(format!(
                "unknown import type '{}'; must be assets, users, or tickets",
                query.job_type
            ));
        }
    };
    if !matches!(job_type, ImportType::Assets) {
        return errors::bad_request(
            "users and tickets imports aren't shipped yet; check back in Phase 2/3",
        );
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Pull the first file field. Like the branding upload we
    // only consume the first multipart field; extra fields are
    // ignored.
    let (filename, file_bytes) = match read_first_file_field(&mut payload).await {
        Ok(v) => v,
        Err(e) => return errors::bad_request(e),
    };
    if file_bytes.is_empty() {
        return errors::bad_request("uploaded file is empty");
    }

    // Persist the raw upload under UPLOAD_DIR/imports/{uuid}.csv
    // so the commit step can re-read the same bytes the dry-run
    // saw. UUID prefix is the import_jobs.id we're about to mint.
    let job_id = Uuid::new_v4();
    let stored_path = match persist_upload(job_id, &filename, &file_bytes) {
        Ok(p) => p,
        Err(e) => {
            error!(error = ?e, "failed to persist import upload");
            return errors::internal("failed to save uploaded file");
        }
    };

    let job = match repo::create(
        &mut conn,
        NewImportJob {
            job_type: job_type.as_str().to_string(),
            filename: filename.clone(),
            file_path: stored_path.to_string_lossy().to_string(),
            created_by: Some(user_uuid),
        },
    ) {
        Ok(j) => j,
        Err(e) => {
            error!(error = ?e, "failed to create import_jobs row");
            return errors::internal("failed to start import");
        }
    };

    // Parse + dry-run.
    let parsed = match csv_parser::parse_file(&stored_path) {
        Ok(p) => p,
        Err(e) => {
            let message = e.to_string();
            mark_failed(&mut conn, job.id, &message);
            return errors::bad_request(message);
        }
    };

    let summary = match imports::dry_run(&mut conn, job_type, &parsed) {
        Ok(s) => s,
        Err(e) => {
            error!(job_id = %job.id, error = ?e, "dry-run failed");
            mark_failed(&mut conn, job.id, "dry-run failed; see server logs");
            return errors::internal("dry-run failed");
        }
    };

    let summary_value = serde_json::to_value(&summary).unwrap_or(serde_json::Value::Null);
    match repo::update(
        &mut conn,
        job.id,
        ImportJobUpdate {
            status: Some("dry_run_done".to_string()),
            summary: Some(Some(summary_value)),
            ..Default::default()
        },
    ) {
        Ok(updated) => {
            info!(
                job_id = %updated.id,
                rows = summary.row_count,
                errors = summary.errors.len(),
                "import dry-run complete"
            );
            HttpResponse::Ok().json(updated)
        }
        Err(e) => {
            error!(job_id = %job.id, error = ?e, "failed to save dry-run summary");
            errors::internal("failed to save dry-run summary")
        }
    }
}

/// Apply a previously dry-run'd job. Idempotent on status: a
/// job already in `done` returns the existing row unchanged.
pub async fn commit(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(c) => c.clone(),
        None => return errors::unauthorized("Authentication required"),
    };
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let id = path.into_inner();
    let job = match repo::get(&mut conn, id) {
        Ok(j) => j,
        Err(diesel::result::Error::NotFound) => {
            return errors::not_found_msg(format!("import job {id} not found"));
        }
        Err(e) => {
            error!(error = ?e, "failed to load import job");
            return errors::internal("failed to load import job");
        }
    };
    if job.status == "done" {
        return HttpResponse::Ok().json(job);
    }
    if job.status != "dry_run_done" {
        return errors::bad_request(format!(
            "job is in status '{}'; commit requires 'dry_run_done'",
            job.status
        ));
    }

    let job_type = match ImportType::from_str(&job.job_type) {
        Some(t) => t,
        None => {
            return errors::internal(format!("import job has unknown type '{}'", job.job_type));
        }
    };

    // Mark committing first so a parallel commit attempt fails
    // the precondition above instead of double-applying.
    if let Err(e) = repo::update(
        &mut conn,
        job.id,
        ImportJobUpdate {
            status: Some("committing".to_string()),
            ..Default::default()
        },
    ) {
        error!(error = ?e, "failed to mark job committing");
        return errors::internal("failed to start commit");
    }

    let parsed = match csv_parser::parse_file(std::path::Path::new(&job.file_path)) {
        Ok(p) => p,
        Err(e) => {
            mark_failed(&mut conn, job.id, &e.to_string());
            return errors::bad_request(e.to_string());
        }
    };

    let committed = conn
        .transaction::<i32, diesel::result::Error, _>(|c| imports::commit(c, job_type, &parsed));
    match committed {
        Ok(count) => {
            match repo::update(
                &mut conn,
                job.id,
                ImportJobUpdate {
                    status: Some("done".to_string()),
                    records_committed: Some(Some(count)),
                    completed_at: Some(Some(chrono::Utc::now())),
                    ..Default::default()
                },
            ) {
                Ok(updated) => HttpResponse::Ok().json(updated),
                Err(e) => {
                    error!(error = ?e, "failed to finalise import job");
                    errors::internal("failed to finalise import job")
                }
            }
        }
        Err(e) => {
            error!(job_id = %job.id, error = ?e, "commit failed");
            mark_failed(&mut conn, job.id, &format!("commit failed: {e}"));
            errors::internal("commit failed")
        }
    }
}

pub async fn get_job(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let _claims = match req.extensions().get::<crate::models::Claims>() {
        Some(c) => c.clone(),
        None => return errors::unauthorized("Authentication required"),
    };
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let id = path.into_inner();
    match repo::get(&mut conn, id) {
        Ok(j) => HttpResponse::Ok().json(j),
        Err(diesel::result::Error::NotFound) => {
            errors::not_found_msg(format!("import job {id} not found"))
        }
        Err(e) => {
            error!(error = ?e, "failed to load import job");
            errors::internal("failed to load import job")
        }
    }
}

/// Plain-text CSV template for the requested import type. Just
/// the header row; the admin fills in the data rows.
pub async fn template(path: web::Path<String>) -> impl Responder {
    let job_type = match ImportType::from_str(&path) {
        Some(t) => t,
        None => return errors::bad_request("unknown import type"),
    };
    if !matches!(job_type, ImportType::Assets) {
        return errors::bad_request(
            "users and tickets templates aren't shipped yet; check back in Phase 2/3",
        );
    }
    let importer = imports::importer_for(job_type);
    let body = importer.template_headers().join(",") + "\n";
    HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "text/csv; charset=utf-8"))
        .insert_header((
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"nosdesk-{}-template.csv\"",
                job_type.as_str()
            ),
        ))
        .body(body)
}

// ---- helpers ----

async fn read_first_file_field(payload: &mut Multipart) -> Result<(String, Vec<u8>), String> {
    while let Ok(Some(mut field)) = payload.try_next().await {
        let filename = field
            .content_disposition()
            .get_filename()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "upload.csv".to_string());

        let mut bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = field.next().await {
            let chunk = chunk.map_err(|e| format!("read error: {e}"))?;
            bytes.extend_from_slice(&chunk);
            // 10 MB cap — well above plumber-friend volumes,
            // well below our memory budget. Larger files belong
            // in the Phase 2 background-worker path.
            if bytes.len() > 10 * 1024 * 1024 {
                return Err("file exceeds the 10 MB upload cap".to_string());
            }
        }
        return Ok((filename, bytes));
    }
    Err("missing file field in upload".to_string())
}

fn persist_upload(
    job_id: Uuid,
    _filename: &str,
    bytes: &[u8],
) -> std::io::Result<std::path::PathBuf> {
    let upload_root = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "/app/uploads".to_string());
    let dir = std::path::Path::new(&upload_root).join("imports");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{job_id}.csv"));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

fn mark_failed(conn: &mut crate::db::DbConnection, id: Uuid, message: &str) {
    if let Err(e) = repo::update(
        conn,
        id,
        ImportJobUpdate {
            status: Some("failed".to_string()),
            error_message: Some(Some(message.to_string())),
            completed_at: Some(Some(chrono::Utc::now())),
            ..Default::default()
        },
    ) {
        error!(error = ?e, job_id = %id, "failed to mark import job failed");
    }
}
