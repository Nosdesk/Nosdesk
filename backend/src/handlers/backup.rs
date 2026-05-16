use crate::handlers::errors;
use crate::handlers::helpers;
use actix_multipart::Multipart;
use actix_web::{web, HttpMessage, HttpResponse, Responder};
use futures::StreamExt;
use serde_json::json;
use std::io::Write;
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{
    BackupJobResponse, BackupJobUpdate, Claims, ExecuteRestoreRequest, NewBackupJob,
    StartBackupExportRequest,
};
use crate::repository::backup as backup_repo;
use crate::services::backup as backup_service;
use crate::utils::image::generate_user_avatar_thumbnail;

/// Start a backup export job
/// POST /api/admin/backup/export
pub async fn start_export(
    pool: web::Data<Pool>,
    req: actix_web::HttpRequest,
    body: web::Json<StartBackupExportRequest>,
) -> impl Responder {
    // Get authenticated admin user
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return errors::forbidden("Admin access required");
    }

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    // Validate password requirement for sensitive data
    if body.include_sensitive && body.password.is_none() {
        return errors::bad_request("Password is required when including sensitive data");
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Create backup job
    let new_job = NewBackupJob {
        job_type: "export".to_string(),
        status: "processing".to_string(),
        include_sensitive: body.include_sensitive,
        created_by: Some(user_uuid),
    };

    let job = match backup_repo::create_backup_job(&mut conn, new_job) {
        Ok(job) => job,
        Err(e) => return errors::internal(format!("Failed to create job: {}", e)),
    };

    let job_id = job.id;
    let include_sensitive = body.include_sensitive;
    let password = body.password.clone();

    // Run backup in background
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        let mut conn = match pool_clone.get() {
            Ok(conn) => conn,
            Err(e) => {
                log::error!("Failed to get database connection for backup: {e}");
                return;
            }
        };

        // `include_sensitive` is gone; the password's presence
        // decides everything. With a password, the whole archive
        // is sealed and sensitive fields are included. Without,
        // the zip is plaintext and sensitive fields are stripped.
        let _ = include_sensitive;
        match backup_service::create_backup(&mut conn, job_id, password.as_deref()) {
            Ok(path) => {
                log::info!("Backup completed successfully: {path:?}");
            }
            Err(e) => {
                log::error!("Backup failed: {e}");
                // Update job with error
                let _ = backup_repo::update_backup_job(
                    &mut conn,
                    job_id,
                    BackupJobUpdate {
                        status: Some("failed".to_string()),
                        file_path: None,
                        file_size: None,
                        error_message: Some(e.to_string()),
                        completed_at: Some(chrono::Utc::now().naive_utc()),
                    },
                );
            }
        }
    });

    HttpResponse::Accepted().json(BackupJobResponse::from(job))
}

/// Get all backup/restore jobs
/// GET /api/admin/backup/jobs
pub async fn get_jobs(pool: web::Data<Pool>, req: actix_web::HttpRequest) -> impl Responder {
    // Get authenticated admin user
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return errors::forbidden("Admin access required");
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match backup_repo::get_all_backup_jobs(&mut conn) {
        Ok(jobs) => {
            let responses: Vec<BackupJobResponse> =
                jobs.into_iter().map(BackupJobResponse::from).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => errors::internal(format!("Failed to get jobs: {}", e)),
    }
}

/// Get a specific backup job
/// GET /api/admin/backup/jobs/{id}
pub async fn get_job(
    pool: web::Data<Pool>,
    path: web::Path<String>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    // Get authenticated admin user
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return errors::forbidden("Admin access required");
    }

    let job_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid job ID"),
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match backup_repo::get_backup_job(&mut conn, job_id) {
        Ok(job) => HttpResponse::Ok().json(BackupJobResponse::from(job)),
        Err(diesel::result::Error::NotFound) => errors::not_found_msg("Job not found"),
        Err(e) => errors::internal(format!("Failed to get job: {}", e)),
    }
}

/// Download a completed backup
/// GET /api/admin/backup/download/{id}
pub async fn download_backup(
    pool: web::Data<Pool>,
    path: web::Path<String>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    // Get authenticated admin user
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return errors::forbidden("Admin access required");
    }

    let job_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid job ID"),
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let job = match backup_repo::get_backup_job(&mut conn, job_id) {
        Ok(job) => job,
        Err(diesel::result::Error::NotFound) => return errors::not_found_msg("Job not found"),
        Err(e) => return errors::internal(format!("Failed to get job: {}", e)),
    };

    if job.status != "completed" {
        return errors::bad_request("Backup not completed");
    }

    let file_path = match job.file_path {
        Some(path) => path,
        None => return errors::bad_request("No backup file available"),
    };

    // Serve the file
    match actix_files::NamedFile::open(&file_path) {
        Ok(file) => {
            let filename = std::path::Path::new(&file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("backup.zip");

            file.set_content_disposition(actix_web::http::header::ContentDisposition {
                disposition: actix_web::http::header::DispositionType::Attachment,
                parameters: vec![actix_web::http::header::DispositionParam::Filename(
                    filename.to_string(),
                )],
            })
            .into_response(&req)
        }
        Err(e) => errors::internal(format!("Failed to read backup file: {}", e)),
    }
}

/// Upload a backup for restore
/// POST /api/admin/backup/restore/upload
pub async fn upload_restore(
    pool: web::Data<Pool>,
    req: actix_web::HttpRequest,
    mut payload: Multipart,
) -> impl Responder {
    // Get authenticated admin user
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return errors::forbidden("Admin access required");
    }

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    // Get upload directory
    let backups_dir = std::env::var("UPLOAD_DIR")
        .map(|d| std::path::PathBuf::from(d).join("backups").join("uploads"))
        .unwrap_or_else(|_| std::path::PathBuf::from("/app/uploads/backups/uploads"));

    if let Err(e) = std::fs::create_dir_all(&backups_dir) {
        return errors::internal(format!("Failed to create upload directory: {}", e));
    }

    // Process the multipart upload
    let mut file_path: Option<std::path::PathBuf> = None;

    while let Some(item) = payload.next().await {
        let mut field = match item {
            Ok(field) => field,
            Err(e) => return errors::bad_request(format!("Upload error: {}", e)),
        };

        let filename = field
            .content_disposition()
            .get_filename()
            .map(sanitize_filename::sanitize)
            .unwrap_or_else(|| format!("restore-{}.zip", Uuid::new_v4()));

        let filepath = backups_dir.join(&filename);

        let mut file = match std::fs::File::create(&filepath) {
            Ok(f) => f,
            Err(e) => return errors::internal(format!("Failed to create file: {}", e)),
        };

        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(data) => data,
                Err(e) => return errors::bad_request(format!("Upload error: {}", e)),
            };
            if let Err(e) = file.write_all(&data) {
                return errors::internal(format!("Failed to write file: {}", e));
            }
        }

        file_path = Some(filepath);
    }

    let filepath = match file_path {
        Some(p) => p,
        None => return errors::bad_request("No file uploaded"),
    };

    // Upload-time validation removed: the manifest now lives
    // inside the encryption envelope, so an encrypted backup
    // can't be parsed without the password (which we don't have
    // here). The restore endpoint validates with the password.

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Create restore job
    let new_job = NewBackupJob {
        job_type: "restore".to_string(),
        status: "pending".to_string(),
        include_sensitive: false, // Will be updated after preview
        created_by: Some(user_uuid),
    };

    let job = match backup_repo::create_backup_job(&mut conn, new_job) {
        Ok(job) => job,
        Err(e) => return errors::internal(format!("Failed to create job: {}", e)),
    };

    // Update job with file path
    let job = match backup_repo::update_backup_job(
        &mut conn,
        job.id,
        BackupJobUpdate {
            status: None,
            file_path: Some(filepath.to_string_lossy().to_string()),
            file_size: Some(
                std::fs::metadata(&filepath)
                    .map(|m| m.len() as i64)
                    .unwrap_or(0),
            ),
            error_message: None,
            completed_at: None,
        },
    ) {
        Ok(job) => job,
        Err(e) => return errors::internal(format!("Failed to update job: {}", e)),
    };

    HttpResponse::Created().json(BackupJobResponse::from(job))
}

/// Preview restore contents
/// GET /api/admin/backup/restore/{id}/preview
pub async fn preview_restore(
    pool: web::Data<Pool>,
    path: web::Path<String>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    // Get authenticated admin user
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return errors::forbidden("Admin access required");
    }

    let job_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid job ID"),
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let job = match backup_repo::get_backup_job(&mut conn, job_id) {
        Ok(job) => job,
        Err(diesel::result::Error::NotFound) => return errors::not_found_msg("Job not found"),
        Err(e) => return errors::internal(format!("Failed to get job: {}", e)),
    };

    let file_path = match job.file_path {
        Some(path) => std::path::PathBuf::from(path),
        None => return errors::bad_request("No backup file available"),
    };

    // GET preview has no body to carry a password; encrypted
    // backups will fail here with "password required" and the
    // operator can drive the actual restore via POST which does
    // take the password.
    match backup_service::preview_restore(&file_path, None) {
        Ok(preview) => HttpResponse::Ok().json(preview),
        Err(e) => errors::internal(format!("Failed to preview: {}", e)),
    }
}

/// Execute restore
/// POST /api/admin/backup/restore/{id}/execute
pub async fn execute_restore(
    pool: web::Data<Pool>,
    path: web::Path<String>,
    req: actix_web::HttpRequest,
    body: web::Json<ExecuteRestoreRequest>,
) -> impl Responder {
    // Get authenticated admin user
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return errors::forbidden("Admin access required");
    }

    let job_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid job ID"),
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let job = match backup_repo::get_backup_job(&mut conn, job_id) {
        Ok(job) => job,
        Err(diesel::result::Error::NotFound) => return errors::not_found_msg("Job not found"),
        Err(e) => return errors::internal(format!("Failed to get job: {}", e)),
    };

    if job.job_type != "restore" {
        return errors::bad_request("Job is not a restore job");
    }

    let file_path = match job.file_path {
        Some(path) => std::path::PathBuf::from(path),
        None => return errors::bad_request("No backup file available"),
    };

    // Preview now drives password verification too — a
    // successful preview means decryption worked. Encrypted
    // backups without a password fail here with a clear
    // "password required" error; wrong-password backups fail
    // with a decryption error.
    if let Err(e) = backup_service::preview_restore(&file_path, body.password.as_deref()) {
        return errors::bad_request(format!("Preview failed: {}", e));
    }

    // Update job status
    let _ = backup_repo::update_backup_job(
        &mut conn,
        job_id,
        BackupJobUpdate {
            status: Some("processing".to_string()),
            file_path: None,
            file_size: None,
            error_message: None,
            completed_at: None,
        },
    );

    // Restore database first, then files. Mirrors the onboarding-only
    // `setup_restore_execute` flow below — the two paths now share the
    // same restore semantics, differing only in their auth gate
    // (admin claims here vs zero-users-on-system there).
    let stats = match backup_service::restore_database(
        &mut conn,
        &file_path,
        body.password.as_deref(),
        // Admin auth is the upstream gate for this endpoint; the
        // operator explicitly chose to restore over the live DB.
        backup_service::RestoreOptions {
            force_non_empty: true,
            ignore_schema_mismatch: false,
        },
    ) {
        Ok(s) => s,
        Err(e) => {
            let _ = backup_repo::update_backup_job(
                &mut conn,
                job_id,
                BackupJobUpdate {
                    status: Some("failed".to_string()),
                    file_path: None,
                    file_size: None,
                    error_message: Some(format!("database restore failed: {e}")),
                    completed_at: Some(chrono::Utc::now().naive_utc()),
                },
            );
            return errors::internal(format!("Database restore failed: {e}"));
        }
    };

    // Files restore is best-effort: a missing or partial files payload
    // shouldn't undo the database restore that just completed.
    let files_restored =
        match backup_service::restore_backup_files(&file_path, body.password.as_deref()) {
            Ok(count) => count,
            Err(e) => {
                tracing::warn!(error = %e, "File restore had issues during admin restore");
                0
            }
        };

    let thumbnails_regenerated = regenerate_user_thumbnails(&mut conn).await;

    let _ = backup_repo::update_backup_job(
        &mut conn,
        job_id,
        BackupJobUpdate {
            status: Some("completed".to_string()),
            file_path: None,
            file_size: None,
            error_message: None,
            completed_at: Some(chrono::Utc::now().naive_utc()),
        },
    );

    HttpResponse::Ok().json(json!({
        "success": true,
        "tables_restored": stats.tables_restored,
        "records_restored": stats.records_restored,
        "files_restored": files_restored,
        "thumbnails_regenerated": thumbnails_regenerated,
    }))
}

/// Delete a backup job and its associated file
/// DELETE /api/admin/backup/jobs/{id}
pub async fn delete_job(
    pool: web::Data<Pool>,
    path: web::Path<String>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    // Get authenticated admin user
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return errors::forbidden("Admin access required");
    }

    let job_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid job ID"),
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get job first to delete associated file
    let job = match backup_repo::get_backup_job(&mut conn, job_id) {
        Ok(job) => job,
        Err(diesel::result::Error::NotFound) => return errors::not_found_msg("Job not found"),
        Err(e) => return errors::internal(format!("Failed to get job: {}", e)),
    };

    // Delete associated file if exists
    if let Some(file_path) = &job.file_path {
        if let Err(e) = backup_service::delete_backup_file(file_path) {
            log::warn!("Failed to delete backup file: {e}");
        }
    }

    // Delete job from database
    match backup_repo::delete_backup_job(&mut conn, job_id) {
        Ok(_) => HttpResponse::Ok().json(json!({"success": true, "message": "Job deleted"})),
        Err(e) => errors::internal(format!("Failed to delete job: {}", e)),
    }
}

// AUD-005: unauthenticated onboarding-restore endpoints removed.
// Restore now ships via `nosdesk-cli db restore`, gated on shell
// access. Authed admin restore at /api/admin/backup/restore/*
// is unchanged.

/// Regenerate thumbnails for all users with avatars
/// Returns the count of successfully regenerated thumbnails
async fn regenerate_user_thumbnails(conn: &mut crate::db::DbConnection) -> u64 {
    use diesel::RunQueryDsl;

    // Query all users with avatar URLs
    #[derive(diesel::QueryableByName)]
    struct UserAvatar {
        #[diesel(sql_type = diesel::sql_types::Text)]
        uuid_str: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        avatar: String,
    }

    let user_avatars: Vec<UserAvatar> = match diesel::sql_query(
        "SELECT uuid::text as uuid_str, avatar_url as avatar FROM users WHERE avatar_url IS NOT NULL"
    ).load(conn) {
        Ok(avatars) => avatars,
        Err(e) => {
            log::error!("Failed to query users for thumbnail regeneration: {e}");
            return 0;
        }
    };

    let mut regenerated = 0u64;

    for user_avatar in user_avatars {
        match generate_user_avatar_thumbnail(&user_avatar.avatar, &user_avatar.uuid_str).await {
            Ok(Some(_)) => {
                regenerated += 1;
                log::debug!("Regenerated thumbnail for user {}", user_avatar.uuid_str);
            }
            Ok(None) => {
                log::warn!(
                    "Could not generate thumbnail for user {} - avatar may be missing",
                    user_avatar.uuid_str
                );
            }
            Err(e) => {
                log::warn!(
                    "Failed to regenerate thumbnail for user {}: {}",
                    user_avatar.uuid_str,
                    e
                );
            }
        }
    }

    regenerated
}
