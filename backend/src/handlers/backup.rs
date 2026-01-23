use actix_web::{web, HttpResponse, HttpMessage, Responder};
use actix_multipart::Multipart;
use diesel::prelude::*;
use futures::StreamExt;
use serde_json::json;
use std::io::Write;
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{
    Claims, StartBackupExportRequest, ExecuteRestoreRequest, BackupJobResponse,
    NewBackupJob, BackupJobUpdate,
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
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({"error": "Admin access required"}));
    }

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({"error": "Invalid user UUID"})),
    };

    // Validate password requirement for sensitive data
    if body.include_sensitive && body.password.is_none() {
        return HttpResponse::BadRequest().json(json!({
            "error": "Password is required when including sensitive data"
        }));
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Database error: {}", e)})),
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
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to create job: {}", e)})),
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

        match backup_service::create_backup(&mut conn, job_id, include_sensitive, password.as_deref()) {
            Ok(path) => {
                log::info!("Backup completed successfully: {path:?}");
            }
            Err(e) => {
                log::error!("Backup failed: {e}");
                // Update job with error
                let _ = backup_repo::update_backup_job(&mut conn, job_id, BackupJobUpdate {
                    status: Some("failed".to_string()),
                    file_path: None,
                    file_size: None,
                    error_message: Some(e.to_string()),
                    completed_at: Some(chrono::Utc::now().naive_utc()),
                });
            }
        }
    });

    HttpResponse::Accepted().json(BackupJobResponse::from(job))
}

/// Get all backup/restore jobs
/// GET /api/admin/backup/jobs
pub async fn get_jobs(
    pool: web::Data<Pool>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    // Get authenticated admin user
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({"error": "Admin access required"}));
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Database error: {}", e)})),
    };

    match backup_repo::get_all_backup_jobs(&mut conn) {
        Ok(jobs) => {
            let responses: Vec<BackupJobResponse> = jobs.into_iter().map(BackupJobResponse::from).collect();
            HttpResponse::Ok().json(responses)
        }
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": format!("Failed to get jobs: {}", e)})),
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
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({"error": "Admin access required"}));
    }

    let job_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({"error": "Invalid job ID"})),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Database error: {}", e)})),
    };

    match backup_repo::get_backup_job(&mut conn, job_id) {
        Ok(job) => HttpResponse::Ok().json(BackupJobResponse::from(job)),
        Err(diesel::result::Error::NotFound) => HttpResponse::NotFound().json(json!({"error": "Job not found"})),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": format!("Failed to get job: {}", e)})),
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
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({"error": "Admin access required"}));
    }

    let job_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({"error": "Invalid job ID"})),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Database error: {}", e)})),
    };

    let job = match backup_repo::get_backup_job(&mut conn, job_id) {
        Ok(job) => job,
        Err(diesel::result::Error::NotFound) => return HttpResponse::NotFound().json(json!({"error": "Job not found"})),
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to get job: {}", e)})),
    };

    if job.status != "completed" {
        return HttpResponse::BadRequest().json(json!({"error": "Backup not completed"}));
    }

    let file_path = match job.file_path {
        Some(path) => path,
        None => return HttpResponse::BadRequest().json(json!({"error": "No backup file available"})),
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
                parameters: vec![actix_web::http::header::DispositionParam::Filename(filename.to_string())],
            })
            .into_response(&req)
        }
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": format!("Failed to read backup file: {}", e)})),
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
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({"error": "Admin access required"}));
    }

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({"error": "Invalid user UUID"})),
    };

    // Get upload directory
    let backups_dir = std::env::var("UPLOAD_DIR")
        .map(|d| std::path::PathBuf::from(d).join("backups").join("uploads"))
        .unwrap_or_else(|_| std::path::PathBuf::from("/app/uploads/backups/uploads"));

    if let Err(e) = std::fs::create_dir_all(&backups_dir) {
        return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to create upload directory: {}", e)}));
    }

    // Process the multipart upload
    let mut file_path: Option<std::path::PathBuf> = None;

    while let Some(item) = payload.next().await {
        let mut field = match item {
            Ok(field) => field,
            Err(e) => return HttpResponse::BadRequest().json(json!({"error": format!("Upload error: {}", e)})),
        };

        let filename = field
            .content_disposition()
            .get_filename()
            .map(sanitize_filename::sanitize)
            .unwrap_or_else(|| format!("restore-{}.zip", Uuid::new_v4()));

        let filepath = backups_dir.join(&filename);

        let mut file = match std::fs::File::create(&filepath) {
            Ok(f) => f,
            Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to create file: {}", e)})),
        };

        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(data) => data,
                Err(e) => return HttpResponse::BadRequest().json(json!({"error": format!("Upload error: {}", e)})),
            };
            if let Err(e) = file.write_all(&data) {
                return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to write file: {}", e)}));
            }
        }

        file_path = Some(filepath);
    }

    let filepath = match file_path {
        Some(p) => p,
        None => return HttpResponse::BadRequest().json(json!({"error": "No file uploaded"})),
    };

    // Validate the backup file
    match backup_service::read_backup_manifest(&filepath) {
        Ok(_) => {}
        Err(e) => {
            let _ = std::fs::remove_file(&filepath);
            return HttpResponse::BadRequest().json(json!({"error": format!("Invalid backup file: {}", e)}));
        }
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Database error: {}", e)})),
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
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to create job: {}", e)})),
    };

    // Update job with file path
    let job = match backup_repo::update_backup_job(&mut conn, job.id, BackupJobUpdate {
        status: None,
        file_path: Some(filepath.to_string_lossy().to_string()),
        file_size: Some(std::fs::metadata(&filepath).map(|m| m.len() as i64).unwrap_or(0)),
        error_message: None,
        completed_at: None,
    }) {
        Ok(job) => job,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to update job: {}", e)})),
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
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({"error": "Admin access required"}));
    }

    let job_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({"error": "Invalid job ID"})),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Database error: {}", e)})),
    };

    let job = match backup_repo::get_backup_job(&mut conn, job_id) {
        Ok(job) => job,
        Err(diesel::result::Error::NotFound) => return HttpResponse::NotFound().json(json!({"error": "Job not found"})),
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to get job: {}", e)})),
    };

    let file_path = match job.file_path {
        Some(path) => std::path::PathBuf::from(path),
        None => return HttpResponse::BadRequest().json(json!({"error": "No backup file available"})),
    };

    match backup_service::preview_restore(&file_path) {
        Ok(preview) => HttpResponse::Ok().json(preview),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": format!("Failed to preview: {}", e)})),
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
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({"error": "Admin access required"}));
    }

    let job_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({"error": "Invalid job ID"})),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Database error: {}", e)})),
    };

    let job = match backup_repo::get_backup_job(&mut conn, job_id) {
        Ok(job) => job,
        Err(diesel::result::Error::NotFound) => return HttpResponse::NotFound().json(json!({"error": "Job not found"})),
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to get job: {}", e)})),
    };

    if job.job_type != "restore" {
        return HttpResponse::BadRequest().json(json!({"error": "Job is not a restore job"}));
    }

    let file_path = match job.file_path {
        Some(path) => std::path::PathBuf::from(path),
        None => return HttpResponse::BadRequest().json(json!({"error": "No backup file available"})),
    };

    // Verify password if backup has encrypted sensitive data
    let preview = match backup_service::preview_restore(&file_path) {
        Ok(p) => p,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to preview: {}", e)})),
    };

    if preview.has_encrypted_sensitive {
        match &body.password {
            Some(password) => {
                match backup_service::verify_backup_password(&file_path, password) {
                    Ok(true) => {}
                    Ok(false) => return HttpResponse::BadRequest().json(json!({"error": "Invalid password"})),
                    Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Password verification failed: {}", e)})),
                }
            }
            None => return HttpResponse::BadRequest().json(json!({"error": "Password required for encrypted backup"})),
        }
    }

    // Update job status
    let _ = backup_repo::update_backup_job(&mut conn, job_id, BackupJobUpdate {
        status: Some("processing".to_string()),
        file_path: None,
        file_size: None,
        error_message: None,
        completed_at: None,
    });

    // Restore files (database restore would need more work)
    match backup_service::restore_backup_files(&file_path) {
        Ok(count) => {
            let _ = backup_repo::update_backup_job(&mut conn, job_id, BackupJobUpdate {
                status: Some("completed".to_string()),
                file_path: None,
                file_size: None,
                error_message: None,
                completed_at: Some(chrono::Utc::now().naive_utc()),
            });

            HttpResponse::Ok().json(json!({
                "success": true,
                "files_restored": count,
                "message": "Files restored successfully. Database restore is not yet implemented."
            }))
        }
        Err(e) => {
            let _ = backup_repo::update_backup_job(&mut conn, job_id, BackupJobUpdate {
                status: Some("failed".to_string()),
                file_path: None,
                file_size: None,
                error_message: Some(e.to_string()),
                completed_at: Some(chrono::Utc::now().naive_utc()),
            });

            HttpResponse::InternalServerError().json(json!({"error": format!("Restore failed: {}", e)}))
        }
    }
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
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    // Check if user is admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({"error": "Admin access required"}));
    }

    let job_id = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({"error": "Invalid job ID"})),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Database error: {}", e)})),
    };

    // Get job first to delete associated file
    let job = match backup_repo::get_backup_job(&mut conn, job_id) {
        Ok(job) => job,
        Err(diesel::result::Error::NotFound) => return HttpResponse::NotFound().json(json!({"error": "Job not found"})),
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to get job: {}", e)})),
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
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": format!("Failed to delete job: {}", e)})),
    }
}

// ============================================================================
// ONBOARDING RESTORE ENDPOINTS (Unauthenticated - only work during setup)
// ============================================================================

/// Check if system requires setup (no users exist)
fn check_requires_setup(conn: &mut crate::db::DbConnection) -> bool {
    use crate::schema::users::dsl::*;
    use diesel::dsl::count_star;

    match users.select(count_star()).first::<i64>(conn) {
        Ok(user_count) => user_count == 0,
        Err(_) => false,
    }
}

/// Upload a backup for onboarding restore (no auth required, only works during setup)
/// POST /api/setup/restore/upload
pub async fn onboarding_upload_restore(
    pool: web::Data<Pool>,
    mut payload: Multipart,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Database error: {}", e)})),
    };

    // Security check: only allow during initial setup
    if !check_requires_setup(&mut conn) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Restore via onboarding is only allowed during initial setup"
        }));
    }

    // Get upload directory
    let backups_dir = std::env::var("UPLOAD_DIR")
        .map(|d| std::path::PathBuf::from(d).join("backups").join("uploads"))
        .unwrap_or_else(|_| std::path::PathBuf::from("/app/uploads/backups/uploads"));

    if let Err(e) = std::fs::create_dir_all(&backups_dir) {
        return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to create upload directory: {}", e)}));
    }

    // Process the multipart upload
    let mut file_path: Option<std::path::PathBuf> = None;

    while let Some(item) = payload.next().await {
        let mut field = match item {
            Ok(field) => field,
            Err(e) => return HttpResponse::BadRequest().json(json!({"error": format!("Upload error: {}", e)})),
        };

        let filename = field
            .content_disposition()
            .get_filename()
            .map(sanitize_filename::sanitize)
            .unwrap_or_else(|| format!("onboarding-restore-{}.zip", Uuid::new_v4()));

        let filepath = backups_dir.join(&filename);

        let mut file = match std::fs::File::create(&filepath) {
            Ok(f) => f,
            Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to create file: {}", e)})),
        };

        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(data) => data,
                Err(e) => return HttpResponse::BadRequest().json(json!({"error": format!("Upload error: {}", e)})),
            };
            if let Err(e) = file.write_all(&data) {
                return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to write file: {}", e)}));
            }
        }

        file_path = Some(filepath);
    }

    let filepath = match file_path {
        Some(p) => p,
        None => return HttpResponse::BadRequest().json(json!({"error": "No file uploaded"})),
    };

    // Validate and get preview
    let preview = match backup_service::preview_restore(&filepath) {
        Ok(p) => p,
        Err(e) => {
            let _ = std::fs::remove_file(&filepath);
            return HttpResponse::BadRequest().json(json!({"error": format!("Invalid backup file: {}", e)}));
        }
    };

    // Return preview along with file path (stored temporarily)
    HttpResponse::Ok().json(json!({
        "file_path": filepath.to_string_lossy().to_string(),
        "preview": preview
    }))
}

/// Execute onboarding restore (no auth required, only works during setup)
/// POST /api/setup/restore/execute
pub async fn onboarding_execute_restore(
    pool: web::Data<Pool>,
    body: web::Json<OnboardingRestoreRequest>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Database error: {}", e)})),
    };

    // Security check: only allow during initial setup
    if !check_requires_setup(&mut conn) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Restore via onboarding is only allowed during initial setup"
        }));
    }

    let file_path = std::path::PathBuf::from(&body.file_path);

    // Verify file exists
    if !file_path.exists() {
        return HttpResponse::BadRequest().json(json!({"error": "Backup file not found. Please upload again."}));
    }

    // Verify password if backup has encrypted sensitive data
    let preview = match backup_service::preview_restore(&file_path) {
        Ok(p) => p,
        Err(e) => return HttpResponse::BadRequest().json(json!({"error": format!("Invalid backup file: {}", e)})),
    };

    if preview.has_encrypted_sensitive {
        match &body.password {
            Some(password) => {
                match backup_service::verify_backup_password(&file_path, password) {
                    Ok(true) => {}
                    Ok(false) => return HttpResponse::BadRequest().json(json!({"error": "Invalid password"})),
                    Err(e) => return HttpResponse::InternalServerError().json(json!({"error": format!("Password verification failed: {}", e)})),
                }
            }
            None => return HttpResponse::BadRequest().json(json!({"error": "Password required for encrypted backup"})),
        }
    }

    // Restore database data using the shared service function
    match backup_service::restore_database(&mut conn, &file_path, body.password.as_deref()) {
        Ok(stats) => {
            // Restore files
            let files_restored = match backup_service::restore_backup_files(&file_path) {
                Ok(count) => count,
                Err(e) => {
                    log::warn!("File restore had issues: {e}");
                    0
                }
            };

            // Regenerate thumbnails for all users with avatars
            let thumbnails_regenerated = regenerate_user_thumbnails(&mut conn).await;
            log::info!("Regenerated {thumbnails_regenerated} user thumbnails after restore");

            // Clean up the uploaded backup file
            let _ = std::fs::remove_file(&file_path);

            HttpResponse::Ok().json(json!({
                "success": true,
                "message": "System restored successfully",
                "tables_restored": stats.tables_restored,
                "records_restored": stats.records_restored,
                "files_restored": files_restored,
                "thumbnails_regenerated": thumbnails_regenerated
            }))
        }
        Err(e) => {
            // Clean up on failure
            let _ = std::fs::remove_file(&file_path);
            HttpResponse::InternalServerError().json(json!({"error": format!("Restore failed: {}", e)}))
        }
    }
}

/// Request body for onboarding restore
#[derive(Debug, serde::Deserialize)]
pub struct OnboardingRestoreRequest {
    pub file_path: String,
    pub password: Option<String>,
}

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
                log::warn!("Could not generate thumbnail for user {} - avatar may be missing", user_avatar.uuid_str);
            }
            Err(e) => {
                log::warn!("Failed to regenerate thumbnail for user {}: {}", user_avatar.uuid_str, e);
            }
        }
    }

    regenerated
}
