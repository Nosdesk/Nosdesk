//! Admin bulk-import endpoints.
//!
//! - `POST /admin/import?type=assets` accepts a multipart CSV
//!   upload, parses it, keeps its grid, and answers with the job
//!   in status `parsed`: the file's headers with samples and a
//!   suggested mapping.
//! - `PUT  /admin/import/{id}/mapping` stores the admin's column
//!   mapping and options, runs the dry run, and moves the job to
//!   `dry_run_done`. Re-runnable until the commit.
//! - `POST /admin/import/{id}/commit` applies the validated
//!   rows in a single transaction and flips status to `done`.
//! - `GET  /admin/import/{id}` returns the job row + summary so
//!   the admin can resume.
//! - `GET  /admin/import/fields?type=` lists the fields a column
//!   can map to.
//! - `GET  /admin/import/template/{type}` returns a CSV header
//!   row the admin can fill in.
//!
//! Every route is workspace-admin only, and files are read before any
//! transaction opens.

use actix_multipart::Multipart;
use actix_web::{http::header, web, HttpResponse, Responder};
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

use std::sync::Arc;

use crate::errors;
use crate::extractors::{AuthContext, TenantConn};
use crate::models::{ImportJob, ImportJobResponse, ImportJobUpdate, NewImportJob};
use crate::repository::imports as repo;
use crate::services::imports::mapping::{self, ColumnMapping, ImportSource, StoredSource};
use crate::services::imports::{
    self, csv_parser, CommitError, FieldSpec, ImportContext, ImportOptions, ImportType,
    ImportedRecords,
};
use crate::services::search::{indexing_tasks, SearchService};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/import",
        web::post().to(crate::handlers::imports::upload),
    )
    // Before `/admin/import/{id}`, which would take "fields" as an id.
    .route(
        "/admin/import/fields",
        web::get().to(crate::handlers::imports::fields),
    )
    .route(
        "/admin/import/template/{type}",
        web::get().to(crate::handlers::imports::template),
    )
    .route(
        "/admin/import/{id}/mapping",
        web::put().to(crate::handlers::imports::put_mapping),
    )
    .route(
        "/admin/import/{id}/commit",
        web::post().to(crate::handlers::imports::commit),
    )
    .route(
        "/admin/import/{id}",
        web::get().to(crate::handlers::imports::get_job),
    );
}

#[derive(Debug, Deserialize)]
pub struct UploadQuery {
    #[serde(rename = "type")]
    pub job_type: String,
}

fn parse_type(raw: &str) -> Result<ImportType, String> {
    ImportType::from_str(raw)
        .ok_or_else(|| format!("unknown import type '{raw}'; must be assets, users, or tickets"))
}

/// What reading an upload came to, before any transaction.
enum ReadOutcome {
    Read(ImportSource),
    /// The file can't be imported; the message is for the admin.
    Refused(String),
}

/// Result variants for the upload flow.
enum UploadOutcome {
    Ok(ImportJob),
    BadRequest(String),
}

/// Upload + parse: store the file and its grid, describe its columns and
/// suggest a mapping. The job waits in `parsed` for the mapping.
pub async fn upload(
    mut tc: TenantConn,
    auth: AuthContext,
    query: web::Query<UploadQuery>,
    mut payload: Multipart,
) -> impl Responder {
    // Bulk import can create/update users (including their roles),
    // assets, and tickets. Restrict to workspace admins. See
    // security-audit-2026-06.
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let user_uuid = auth.user_uuid;

    let job_type = match parse_type(query.job_type.as_str()) {
        Ok(t) => t,
        Err(message) => return errors::bad_request(message),
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

    // The raw upload and its grid are both named by the job id.
    let job_id = Uuid::new_v4();
    let stored_path = match persist_upload(job_id, &file_bytes) {
        Ok(p) => p,
        Err(e) => {
            error!(error = ?e, "failed to persist import upload");
            return errors::internal("failed to save uploaded file");
        }
    };

    // Parse and write the grid off the async worker and before any
    // transaction opens.
    let read = web::block(move || -> std::io::Result<ReadOutcome> {
        let parsed = match csv_parser::parse_bytes(&file_bytes) {
            Ok(p) => p,
            Err(e) => return Ok(ReadOutcome::Refused(e.to_string())),
        };
        if parsed.rows.is_empty() {
            return Ok(ReadOutcome::Refused(
                "the file has a header row and no data".to_string(),
            ));
        }
        mapping::write_grid(job_id, &parsed)?;
        Ok(ReadOutcome::Read(mapping::csv_source(&parsed)))
    })
    .await;
    let read = match read {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => {
            error!(error = ?e, "failed to store import grid");
            return errors::internal("failed to save uploaded file");
        }
        Err(e) => {
            error!(error = ?e, "import parse task failed");
            return errors::internal("failed to read uploaded file");
        }
    };

    let job_type_str = job_type.as_str().to_string();
    let stored_path_str = stored_path.to_string_lossy().to_string();
    let result = tc.run(|conn| {
        let job = repo::create(
            conn,
            NewImportJob {
                id: job_id,
                job_type: job_type_str.clone(),
                filename: stored_filename(&filename),
                file_path: stored_path_str,
                created_by: Some(user_uuid),
            },
        )?;

        let source = match read {
            ReadOutcome::Read(source) => source,
            ReadOutcome::Refused(message) => {
                mark_failed(conn, job.id, &message);
                return Ok(UploadOutcome::BadRequest(message));
            }
        };

        let importer = imports::importer_for(job_type);
        let fields = importer.fields(conn)?;
        let previous = repo::latest_with_mapping(conn, &job_type_str)?
            .and_then(|prev| previous_choices(&prev))
            .unwrap_or_default();
        let suggested_mapping =
            mapping::suggest(&source.headers, &fields, importer.synonyms(), &previous);
        let stored = StoredSource {
            source,
            suggested_mapping,
        };

        let updated = repo::update(
            conn,
            job.id,
            ImportJobUpdate {
                status: Some("parsed".to_string()),
                source: Some(serde_json::to_value(&stored).ok()),
                ..Default::default()
            },
        )?;
        info!(
            job_id = %updated.id,
            rows = stored.source.row_count,
            "import parsed"
        );
        Ok(UploadOutcome::Ok(updated))
    });

    match result {
        Ok(UploadOutcome::Ok(job)) => HttpResponse::Ok().json(ImportJobResponse::from(job)),
        Ok(UploadOutcome::BadRequest(msg)) => errors::bad_request(msg),
        Err(e) => {
            error!(error = ?e, "import upload failed");
            errors::internal("failed to start import")
        }
    }
}

/// A previous job's mapping as normalized header to field id.
fn previous_choices(job: &ImportJob) -> Option<std::collections::HashMap<String, String>> {
    let stored: StoredSource = serde_json::from_value(job.source.clone()?).ok()?;
    let mapping: ColumnMapping = serde_json::from_value(job.mapping.clone()?).ok()?;
    Some(mapping::previous_choices(&stored.source, &mapping))
}

/// The job's stored source, if it has one that reads.
fn stored_source(job: &ImportJob) -> Option<StoredSource> {
    serde_json::from_value(job.source.clone()?).ok()
}

#[derive(Debug, Deserialize)]
pub struct FieldsQuery {
    #[serde(rename = "type")]
    pub job_type: String,
}

/// The fields a column can map to, in picker order.
pub async fn fields(
    mut tc: TenantConn,
    auth: AuthContext,
    query: web::Query<FieldsQuery>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let job_type = match parse_type(query.job_type.as_str()) {
        Ok(t) => t,
        Err(message) => return errors::bad_request(message),
    };
    match tc.run(|conn| imports::importer_for(job_type).fields(conn)) {
        Ok(fields) => HttpResponse::Ok().json(serde_json::json!({ "fields": fields })),
        Err(e) => {
            error!(error = ?e, "failed to list import fields");
            errors::internal("failed to list import fields")
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingRequest {
    #[serde(deserialize_with = "mapping::deserialize_mapping")]
    pub mapping: ColumnMapping,
    #[serde(default)]
    pub options: ImportOptions,
}

/// What checking a mapping against the job came to.
enum MappingCheck {
    Ok {
        job_type: ImportType,
        fields: Vec<FieldSpec>,
        stored: StoredSource,
        options: ImportOptions,
    },
    NotFound,
    BadRequest(String),
    Conflict(String),
}

/// Result variants for the mapping flow.
enum MappingOutcome {
    Ok(ImportJob),
    NotFound,
    Conflict(String),
}

/// Store the admin's mapping and options and run the dry run. Allowed while
/// the job is `parsed` or `dry_run_done`, so the admin can go back to Map,
/// change a choice and check again.
pub async fn put_mapping(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
    body: web::Json<MappingRequest>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let MappingRequest {
        mapping: column_mapping,
        options,
    } = body.into_inner();

    let check = tc.run(|conn| {
        let job = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(MappingCheck::NotFound),
            Err(e) => return Err(e),
        };
        if let Some(message) = mapping_precondition(&job) {
            return Ok(MappingCheck::Conflict(message));
        }
        let Some(job_type) = ImportType::from_str(&job.job_type) else {
            return Ok(MappingCheck::BadRequest(format!(
                "import job has unknown type '{}'",
                job.job_type
            )));
        };
        let Some(stored) = stored_source(&job) else {
            return Ok(MappingCheck::BadRequest(
                "this import has no file to map; upload the file again".to_string(),
            ));
        };
        let fields = imports::importer_for(job_type).fields(conn)?;
        let mut options = options;
        if let Err(message) = resolve_options(conn, job_type, &mut options, &stored.source) {
            return Ok(MappingCheck::BadRequest(message));
        }
        if let Err(e) = mapping::validate(
            job_type,
            &column_mapping,
            &options,
            &fields,
            &stored.source.headers,
        ) {
            return Ok(MappingCheck::BadRequest(e.0));
        }
        Ok(MappingCheck::Ok {
            job_type,
            fields,
            stored,
            options,
        })
    });
    let (job_type, fields, stored, options) = match check {
        Ok(MappingCheck::Ok {
            job_type,
            fields,
            stored,
            options,
        }) => (job_type, fields, stored, options),
        Ok(MappingCheck::NotFound) => {
            return errors::not_found_msg(format!("import job {id} not found"))
        }
        Ok(MappingCheck::BadRequest(message)) => return errors::bad_request(message),
        Ok(MappingCheck::Conflict(message)) => return errors::conflict(message),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to load import job");
            return errors::internal("failed to load import job");
        }
    };

    let grid = match read_job_grid(id).await {
        Ok(grid) => grid,
        Err(resp) => return resp,
    };
    let mapped = mapping::apply_mapping(&grid, &column_mapping, &fields);
    let options_json = serde_json::to_value(&options).ok();
    let ctx = ImportContext {
        actor: Some(auth.user_uuid),
        job_id: id,
        options,
    };

    let result = tc.run(|conn| {
        // Checked again under the transaction: a commit may have started
        // since the read above.
        let job = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(MappingOutcome::NotFound),
            Err(e) => return Err(e),
        };
        if let Some(message) = mapping_precondition(&job) {
            return Ok(MappingOutcome::Conflict(message));
        }
        let mut summary = imports::dry_run(conn, job_type, &mapped, &ctx)?;
        mapping::annotate(&mut summary, &column_mapping, &stored.source.headers);
        let updated = repo::update(
            conn,
            id,
            ImportJobUpdate {
                status: Some("dry_run_done".to_string()),
                summary: Some(serde_json::to_value(&summary).ok()),
                mapping: Some(serde_json::to_value(&column_mapping).ok()),
                options: Some(options_json.clone()),
                ..Default::default()
            },
        )?;
        info!(
            job_id = %id,
            rows = summary.row_count,
            errors = summary.errors_total,
            "import dry-run complete"
        );
        Ok(MappingOutcome::Ok(updated))
    });

    match result {
        Ok(MappingOutcome::Ok(job)) => HttpResponse::Ok().json(ImportJobResponse::from(job)),
        Ok(MappingOutcome::NotFound) => errors::not_found_msg(format!("import job {id} not found")),
        Ok(MappingOutcome::Conflict(message)) => errors::conflict(message),
        Err(e) => {
            error!(job_id = %id, error = ?e, "import dry-run failed");
            errors::internal("failed to check the import")
        }
    }
}

/// Why a job's mapping can't change now, if it can't.
fn mapping_precondition(job: &ImportJob) -> Option<String> {
    match job.status.as_str() {
        "parsed" | "dry_run_done" => None,
        status => Some(format!(
            "job is in status '{status}'; the mapping can change only before the import is committed"
        )),
    }
}

/// Settle the options against the workspace and the file: the default
/// kind must be a kind (stored as its slug), and the decimal mark
/// defaults from the file's delimiter.
fn resolve_options(
    conn: &mut crate::db::DbConnection,
    job_type: ImportType,
    options: &mut ImportOptions,
    source: &ImportSource,
) -> Result<(), String> {
    if options.decimal.is_none() {
        options.decimal = Some(mapping::default_decimal(source));
    }
    let default_kind = options
        .default_kind
        .take()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    if let Some(kind) = default_kind {
        if job_type != ImportType::Assets {
            return Err("default_kind applies to asset imports only".to_string());
        }
        let kinds = crate::repository::asset_kinds::list_kinds(conn).map_err(|e| {
            error!(error = ?e, "failed to list asset kinds");
            "failed to read asset kinds".to_string()
        })?;
        let lower = kind.to_lowercase();
        let found = kinds
            .iter()
            .find(|k| k.slug.to_lowercase() == lower)
            .or_else(|| kinds.iter().find(|k| k.label.to_lowercase() == lower));
        match found {
            Some(k) => options.default_kind = Some(k.slug.clone()),
            None => return Err(format!("unknown default kind '{kind}'")),
        }
    }
    Ok(())
}

/// Read a job's grid off the async worker.
async fn read_job_grid(id: Uuid) -> Result<csv_parser::ParsedCsv, HttpResponse> {
    match web::block(move || mapping::read_grid(id)).await {
        Ok(Ok(grid)) => Ok(grid),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => Err(errors::bad_request(
            "this import's file is no longer available; upload it again",
        )),
        Ok(Err(e)) => {
            error!(job_id = %id, error = ?e, "failed to read import grid");
            Err(errors::internal("failed to read import file"))
        }
        Err(e) => {
            error!(job_id = %id, error = ?e, "import read task failed");
            Err(errors::internal("failed to read import file"))
        }
    }
}

/// Result variants for the commit flow.
enum CommitOutcome {
    Ok(ImportJob, ImportedRecords),
    AlreadyDone(ImportJob),
    NotFound,
    BadRequest(String),
}

/// Apply a previously dry-run'd job with its stored mapping and options.
/// Idempotent on status: a job already in `done` returns the existing row
/// unchanged.
pub async fn commit(
    mut tc: TenantConn,
    auth: AuthContext,
    search_service: web::Data<Arc<SearchService>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    // Committing an import applies row writes (roles included).
    // Workspace-admin only. See security-audit-2026-06.
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();

    // Read the job and its grid before the commit transaction opens.
    let loaded = tc.run(|conn| {
        let job = repo::get(conn, id)?;
        let fields = match ImportType::from_str(&job.job_type) {
            Some(t) => imports::importer_for(t).fields(conn)?,
            None => Vec::new(),
        };
        Ok((job, fields))
    });
    let (job, fields) = match loaded {
        Ok(v) => v,
        Err(diesel::result::Error::NotFound) => {
            return errors::not_found_msg(format!("import job {id} not found"));
        }
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to load import job");
            return errors::internal("failed to load import job");
        }
    };
    if let Some(outcome) = commit_precondition(&job) {
        return commit_response(&search_service, outcome);
    }
    let Some((column_mapping, options)) = stored_choices(&job) else {
        return errors::bad_request(
            "This import was checked before column mapping and can't be committed. Upload the file again.",
        );
    };
    let ctx = ImportContext {
        actor: Some(auth.user_uuid),
        job_id: id,
        options,
    };
    let grid = match read_job_grid(id).await {
        Ok(grid) => grid,
        Err(resp) => return resp,
    };
    let parsed = mapping::apply_mapping(&grid, &column_mapping, &fields);

    let result = tc.run_result(|conn| -> Result<CommitOutcome, CommitError> {
        // Checked again under the transaction: a parallel commit or a new
        // mapping may have landed since the read above.
        let current = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(CommitOutcome::NotFound),
            Err(e) => return Err(e.into()),
        };
        if let Some(outcome) = commit_precondition(&current) {
            return Ok(outcome);
        }
        if current.mapping != job.mapping || current.options != job.options {
            return Ok(CommitOutcome::BadRequest(
                "the mapping changed while importing; check the rows again".to_string(),
            ));
        }
        let job_type =
            ImportType::from_str(&current.job_type).expect("checked by commit_precondition");

        // Mark committing first so a parallel commit attempt fails
        // the precondition above instead of double-applying.
        repo::update(
            conn,
            current.id,
            ImportJobUpdate {
                status: Some("committing".to_string()),
                ..Default::default()
            },
        )?;

        // The repo + commit imports run inside this transaction.
        // Postgres `SET LOCAL` propagates into the nested savepoint
        // so the workspace GUC is still active for the commit path.
        let records = imports::commit(conn, job_type, &parsed, &ctx)?;

        let updated = repo::update(
            conn,
            current.id,
            ImportJobUpdate {
                status: Some("done".to_string()),
                records_committed: Some(Some(records.len() as i32)),
                completed_at: Some(Some(chrono::Utc::now())),
                ..Default::default()
            },
        )?;
        Ok(CommitOutcome::Ok(updated, records))
    });

    match result {
        Ok(outcome) => commit_response(&search_service, outcome),
        Err(e) => {
            // The commit rolled back every row, and the job's status with
            // it; record the failure on the job in a transaction of its own.
            error!(job_id = %id, error = %e, "commit failed");
            let message = match &e {
                CommitError::Row { row, message } => {
                    format!("Nothing was imported. Row {row} could not be saved: {message}")
                }
                CommitError::Db(_) => "The import failed and nothing was imported.".to_string(),
            };
            if let Err(mark_err) = tc.run(|conn| {
                mark_failed(conn, id, &message);
                Ok(())
            }) {
                error!(job_id = %id, error = ?mark_err, "failed to record import failure");
            }
            match e {
                CommitError::Row { .. } => errors::unprocessable_entity(message),
                CommitError::Db(_) => errors::internal(message),
            }
        }
    }
}

/// The mapping and options stored at the Map step; `None` for a job
/// checked before mapping existed.
fn stored_choices(job: &ImportJob) -> Option<(ColumnMapping, ImportOptions)> {
    let column_mapping = serde_json::from_value(job.mapping.clone()?).ok()?;
    let options = match &job.options {
        Some(v) => serde_json::from_value(v.clone()).ok()?,
        None => ImportOptions::default(),
    };
    Some((column_mapping, options))
}

/// Why a job can't be committed now, if it can't. A job already `done`
/// answers with the job unchanged.
fn commit_precondition(job: &ImportJob) -> Option<CommitOutcome> {
    if job.status == "done" {
        return Some(CommitOutcome::AlreadyDone(job.clone()));
    }
    if job.status != "dry_run_done" {
        return Some(CommitOutcome::BadRequest(format!(
            "job is in status '{}'; commit requires 'dry_run_done'",
            job.status
        )));
    }
    if ImportType::from_str(&job.job_type).is_none() {
        return Some(CommitOutcome::BadRequest(format!(
            "import job has unknown type '{}'",
            job.job_type
        )));
    }
    None
}

fn commit_response(
    search_service: &web::Data<Arc<SearchService>>,
    outcome: CommitOutcome,
) -> HttpResponse {
    match outcome {
        CommitOutcome::Ok(job, records) => {
            // Index the committed entities the same way the create
            // handlers do (spawn_index_*). Run post-commit, off the
            // response path; the repository writers already emitted the
            // per-row sync events inside the transaction.
            index_imported_records(search_service.get_ref(), records);
            HttpResponse::Ok().json(ImportJobResponse::from(job))
        }
        CommitOutcome::AlreadyDone(job) => HttpResponse::Ok().json(ImportJobResponse::from(job)),
        CommitOutcome::NotFound => errors::not_found_msg("import job not found"),
        CommitOutcome::BadRequest(msg) => errors::bad_request(msg),
    }
}

/// Index every entity an import committed into the search index, using
/// the same `spawn_index_*` background tasks the create handlers use, so
/// imported rows become searchable identically to API-created ones. The
/// per-row sync events were already emitted in the commit transaction by
/// the repository writers.
fn index_imported_records(search_service: &Arc<SearchService>, records: ImportedRecords) {
    match records {
        ImportedRecords::Users(users) => {
            for u in users {
                indexing_tasks::spawn_index_user(
                    Arc::clone(search_service),
                    u.user,
                    u.primary_email,
                );
            }
        }
        ImportedRecords::Assets(assets) => {
            for asset in assets {
                indexing_tasks::spawn_index_device(Arc::clone(search_service), asset);
            }
        }
        ImportedRecords::Tickets(tickets) => {
            for ticket in tickets {
                indexing_tasks::spawn_index_ticket(Arc::clone(search_service), ticket, None);
            }
        }
    }
}

pub async fn get_job(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> impl Responder {
    // Job rows carry import previews that can include other users'
    // PII. Workspace-admin only. See security-audit-2026-06.
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    match tc.run(|conn| repo::get(conn, id)) {
        Ok(j) => HttpResponse::Ok().json(ImportJobResponse::from(j)),
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
pub async fn template(auth: AuthContext, path: web::Path<String>) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
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

/// The upload's name as stored: `import_jobs.filename` holds 255
/// characters.
fn stored_filename(name: &str) -> String {
    name.chars().take(255).collect()
}

fn persist_upload(job_id: Uuid, bytes: &[u8]) -> std::io::Result<std::path::PathBuf> {
    let dir = mapping::imports_dir();
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
