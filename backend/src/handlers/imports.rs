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
use actix_web::{http::header, web, HttpResponse, Responder};
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::{ImportJob, ImportJobUpdate, NewImportJob};
use crate::repository::imports as repo;
use crate::services::imports::{self, csv_parser, ImportType};

#[derive(Debug, Deserialize)]
pub struct UploadQuery {
    #[serde(rename = "type")]
    pub job_type: String,
}

/// Result variants for the upload-dry-run flow.
enum UploadOutcome {
    Ok(ImportJob),
    BadRequest(String),
}

/// Upload + parse + dry-run.
pub async fn upload(
    mut tc: TenantConn,
    auth: AuthContext,
    query: web::Query<UploadQuery>,
    mut payload: Multipart,
) -> impl Responder {
    let user_uuid = auth.user_uuid;

    let job_type = match ImportType::from_str(query.job_type.as_str()) {
        Some(t) => t,
        None => {
            return errors::bad_request(format!(
                "unknown import type '{}'; must be assets, users, or tickets",
                query.job_type
            ));
        }
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

    let job_type_str = job_type.as_str().to_string();
    let stored_path_str = stored_path.to_string_lossy().to_string();
    let result = tc.run(|conn| {
        let job = repo::create(
            conn,
            NewImportJob {
                job_type: job_type_str,
                filename: filename.clone(),
                file_path: stored_path_str,
                created_by: Some(user_uuid),
            },
        )?;

        // Parse + dry-run.
        let parsed = match csv_parser::parse_file(&stored_path) {
            Ok(p) => p,
            Err(e) => {
                let message = e.to_string();
                mark_failed(conn, job.id, &message);
                return Ok(UploadOutcome::BadRequest(message));
            }
        };

        let summary = match imports::dry_run(conn, job_type, &parsed) {
            Ok(s) => s,
            Err(e) => {
                error!(job_id = %job.id, error = ?e, "dry-run failed");
                mark_failed(conn, job.id, "dry-run failed; see server logs");
                return Err(e);
            }
        };

        let summary_value = serde_json::to_value(&summary).unwrap_or(serde_json::Value::Null);
        let updated = repo::update(
            conn,
            job.id,
            ImportJobUpdate {
                status: Some("dry_run_done".to_string()),
                summary: Some(Some(summary_value)),
                ..Default::default()
            },
        )?;
        info!(
            job_id = %updated.id,
            rows = summary.row_count,
            errors = summary.errors.len(),
            "import dry-run complete"
        );
        Ok(UploadOutcome::Ok(updated))
    });

    match result {
        Ok(UploadOutcome::Ok(job)) => HttpResponse::Ok().json(job),
        Ok(UploadOutcome::BadRequest(msg)) => errors::bad_request(msg),
        Err(e) => {
            error!(error = ?e, "import upload failed");
            errors::internal("failed to start import")
        }
    }
}

/// Result variants for the commit flow.
enum CommitOutcome {
    Ok(ImportJob),
    AlreadyDone(ImportJob),
    NotFound,
    BadRequest(String),
}

/// Apply a previously dry-run'd job. Idempotent on status: a
/// job already in `done` returns the existing row unchanged.
pub async fn commit(
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<Uuid>,
) -> impl Responder {
    let id = path.into_inner();

    let result = tc.run(|conn| {
        let job = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(CommitOutcome::NotFound),
            Err(e) => return Err(e),
        };
        if job.status == "done" {
            return Ok(CommitOutcome::AlreadyDone(job));
        }
        if job.status != "dry_run_done" {
            return Ok(CommitOutcome::BadRequest(format!(
                "job is in status '{}'; commit requires 'dry_run_done'",
                job.status
            )));
        }

        let job_type = match ImportType::from_str(&job.job_type) {
            Some(t) => t,
            None => {
                return Ok(CommitOutcome::BadRequest(format!(
                    "import job has unknown type '{}'",
                    job.job_type
                )));
            }
        };

        // Mark committing first so a parallel commit attempt fails
        // the precondition above instead of double-applying.
        repo::update(
            conn,
            job.id,
            ImportJobUpdate {
                status: Some("committing".to_string()),
                ..Default::default()
            },
        )?;

        let parsed = match csv_parser::parse_file(std::path::Path::new(&job.file_path)) {
            Ok(p) => p,
            Err(e) => {
                let msg = e.to_string();
                mark_failed(conn, job.id, &msg);
                return Ok(CommitOutcome::BadRequest(msg));
            }
        };

        // The repo + commit imports run inside this transaction.
        // Postgres `SET LOCAL` propagates into the nested savepoint
        // so the workspace GUC is still active for the commit path.
        let committed = match imports::commit(conn, job_type, &parsed) {
            Ok(count) => count,
            Err(e) => {
                let msg = format!("commit failed: {e}");
                mark_failed(conn, job.id, &msg);
                return Err(e);
            }
        };

        let updated = repo::update(
            conn,
            job.id,
            ImportJobUpdate {
                status: Some("done".to_string()),
                records_committed: Some(Some(committed)),
                completed_at: Some(Some(chrono::Utc::now())),
                ..Default::default()
            },
        )?;
        Ok(CommitOutcome::Ok(updated))
    });

    match result {
        Ok(CommitOutcome::Ok(job)) | Ok(CommitOutcome::AlreadyDone(job)) => {
            HttpResponse::Ok().json(job)
        }
        Ok(CommitOutcome::NotFound) => errors::not_found_msg(format!("import job {id} not found")),
        Ok(CommitOutcome::BadRequest(msg)) => errors::bad_request(msg),
        Err(e) => {
            error!(job_id = %id, error = ?e, "commit failed");
            errors::internal("commit failed")
        }
    }
}

pub async fn get_job(
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<Uuid>,
) -> impl Responder {
    let id = path.into_inner();
    match tc.run(|conn| repo::get(conn, id)) {
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
    // The wizard ships exactly one file field; extra parts are
    // ignored. Take the first one and return.
    let mut field = match payload.try_next().await {
        Ok(Some(field)) => field,
        _ => return Err("missing file field in upload".to_string()),
    };

    let filename = field
        .content_disposition()
        .get_filename()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "upload.csv".to_string());

    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = field.next().await {
        let chunk = chunk.map_err(|e| format!("read error: {e}"))?;
        bytes.extend_from_slice(&chunk);
        // 10 MB cap, well above typical single-tenant import
        // volumes, well below our memory budget. Larger files
        // belong in the Phase 2 background-worker path.
        if bytes.len() > 10 * 1024 * 1024 {
            return Err("file exceeds the 10 MB upload cap".to_string());
        }
    }
    Ok((filename, bytes))
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
