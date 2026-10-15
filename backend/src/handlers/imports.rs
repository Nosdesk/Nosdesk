//! Admin bulk-import endpoints.
//!
//! - `POST /admin/import?type=assets` accepts a multipart CSV or
//!   .xlsx upload, parses it, keeps its grid, and answers with the
//!   job in status `parsed`: the file's headers with samples and a
//!   suggested mapping (and, for a workbook, its sheets).
//! - `PUT  /admin/import/{id}/source` reads a workbook again from
//!   another sheet or header row, back to `parsed`.
//! - `PUT  /admin/import/{id}/type` imports the same file as another
//!   type, back to `parsed` with fresh suggestions.
//! - `PUT  /admin/import/{id}/mapping` stores the admin's column
//!   mapping, the columns set to Don't import, and the options, runs the
//!   dry run, and moves the job to `dry_run_done`. Re-runnable until the
//!   commit.
//! - `GET  /admin/import/{id}/values` lists the distinct values of each
//!   mapped column whose values link to a closed set or must read as a
//!   number or date, each with how it matched and its answer.
//! - `PUT  /admin/import/{id}/values` stores the admin's answers to those
//!   values and runs the dry run again.
//! - `POST /admin/import/{id}/commit` applies the validated
//!   rows in a single transaction and flips status to `done`.
//! - `GET  /admin/import/{id}` returns the job row + summary so
//!   the admin can resume.
//! - `DELETE /admin/import/{id}` discards a job nothing was imported
//!   from, with its files.
//! - `GET  /admin/import/{id}/rows?filter=&offset=&limit=` pages
//!   through every row's result, from the dry run or the commit.
//! - `GET  /admin/import/{id}/error-report` downloads the skipped
//!   rows as CSV in the file's own layout, with the reasons.
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
use crate::services::imports::values;
use crate::services::imports::xlsx::{self, FileFormat, SheetChoice};
use crate::services::imports::{
    self, csv_parser, report, CommitError, FieldSpec, ImportContext, ImportOptions, ImportType,
    ImportedRecords, RowOutcome, RowPhase, RowResult, StoredRowResults, ValueAnswers,
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
        "/admin/import/{id}/source",
        web::put().to(crate::handlers::imports::put_source),
    )
    .route(
        "/admin/import/{id}/type",
        web::put().to(crate::handlers::imports::put_type),
    )
    .route(
        "/admin/import/{id}/mapping",
        web::put().to(crate::handlers::imports::put_mapping),
    )
    .route(
        "/admin/import/{id}/commit",
        web::post().to(crate::handlers::imports::commit),
    )
    .service(
        web::resource("/admin/import/{id}/values")
            .route(web::get().to(crate::handlers::imports::get_values))
            .route(web::put().to(crate::handlers::imports::put_values)),
    )
    .route(
        "/admin/import/{id}/rows",
        web::get().to(crate::handlers::imports::rows),
    )
    .route(
        "/admin/import/{id}/error-report",
        web::get().to(crate::handlers::imports::error_report),
    )
    .service(
        web::resource("/admin/import/{id}")
            .route(web::get().to(crate::handlers::imports::get_job))
            .route(web::delete().to(crate::handlers::imports::delete_job)),
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

    // A workbook by its first bytes; anything else reads as CSV.
    let format = xlsx::detect(&file_bytes);

    // The raw upload and its grid are both named by the job id.
    let job_id = Uuid::new_v4();
    let extension = format.as_ref().map_or("upload", |f| f.as_str());
    let stored_path = match persist_upload(job_id, extension, &file_bytes) {
        Ok(p) => p,
        Err(e) => {
            error!(error = ?e, "failed to persist import upload");
            return errors::internal("failed to save uploaded file");
        }
    };

    // Parse and write the grid off the async worker and before any
    // transaction opens.
    let read = parse_off_worker(move || -> std::io::Result<ReadOutcome> {
        let format = match format {
            Ok(f) => f,
            Err(refused) => return Ok(ReadOutcome::Refused(refused.0)),
        };
        match format {
            FileFormat::Xlsx => match xlsx::read(&file_bytes, None) {
                Ok(workbook) => {
                    mapping::write_grid(job_id, &workbook.grid)?;
                    Ok(ReadOutcome::Read(workbook.source))
                }
                Err(refused) => Ok(ReadOutcome::Refused(refused.0)),
            },
            FileFormat::Csv => {
                let parsed = match csv_parser::parse_bytes(&file_bytes) {
                    Ok(p) => p,
                    Err(e) => return Ok(ReadOutcome::Refused(e.to_string())),
                };
                if parsed.rows.is_empty() {
                    return Ok(ReadOutcome::Refused(
                        "The file has a header row and no data.".to_string(),
                    ));
                }
                mapping::write_grid(job_id, &parsed)?;
                Ok(ReadOutcome::Read(mapping::csv_source(&parsed)))
            }
        }
    })
    .await;
    let read = match read {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => {
            error!(error = ?e, "failed to store import grid");
            return errors::internal("failed to save uploaded file");
        }
        Err(ParseFailure::TimedOut) => ReadOutcome::Refused(TIMED_OUT.to_string()),
        Err(ParseFailure::Task(e)) => {
            error!(error = %e, "import parse task failed");
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

        let stored = suggest_for(conn, job_type, source)?;

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

/// Longest a file may take to parse.
const PARSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

const TIMED_OUT: &str =
    "Reading this file took too long. Copy the sheet you need into a new file and upload that.";

enum ParseFailure {
    TimedOut,
    Task(String),
}

/// Run a parse on the blocking pool, giving up after [`PARSE_TIMEOUT`].
/// Giving up doesn't stop the thread; the row, column and unpacked-size
/// caps are what bound the work, and this bounds the request.
async fn parse_off_worker<T, F>(f: F) -> Result<T, ParseFailure>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    match tokio::time::timeout(PARSE_TIMEOUT, web::block(f)).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(ParseFailure::Task(e.to_string())),
        Err(_) => Err(ParseFailure::TimedOut),
    }
}

/// The source with the mapping suggested for it: from the type's last
/// mapped import in the workspace, whatever its status, then ids, labels
/// and synonyms.
fn suggest_for(
    conn: &mut crate::db::DbConnection,
    job_type: ImportType,
    source: ImportSource,
) -> Result<StoredSource, diesel::result::Error> {
    let importer = imports::importer_for(job_type);
    let fields = importer.fields(conn)?;
    let memory = match repo::latest_with_mapping(conn, job_type.as_str())? {
        Some(prev) => header_memory(conn, &prev)?,
        None => mapping::HeaderMemory::default(),
    };
    let suggested_mapping =
        mapping::suggest(&source.headers, &fields, importer.synonyms(), &memory);
    Ok(StoredSource {
        source,
        suggested_mapping,
        ignored: Vec::new(),
    })
}

/// What a previous job chose for each header, and who chose it when.
fn header_memory(
    conn: &mut crate::db::DbConnection,
    job: &ImportJob,
) -> Result<mapping::HeaderMemory, diesel::result::Error> {
    let Some(stored) = stored_source(job) else {
        return Ok(mapping::HeaderMemory::default());
    };
    let Some(column_mapping) = job
        .mapping
        .clone()
        .and_then(|m| serde_json::from_value::<ColumnMapping>(m).ok())
    else {
        return Ok(mapping::HeaderMemory::default());
    };
    let by_name = match job.created_by {
        Some(uuid) => match crate::repository::users::get_user_by_uuid(&uuid, conn) {
            Ok(user) => Some(user.name),
            Err(diesel::result::Error::NotFound) => None,
            Err(e) => return Err(e),
        },
        None => None,
    };
    Ok(mapping::HeaderMemory {
        choices: mapping::previous_choices(&stored.source, &column_mapping, &stored.ignored),
        provenance: Some(mapping::Provenance {
            job_id: job.id,
            by: job.created_by,
            by_name,
            at: job.updated_at,
        }),
    })
}

/// The job's stored source, if it has one that reads.
fn stored_source(job: &ImportJob) -> Option<StoredSource> {
    serde_json::from_value(job.source.clone()?).ok()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRequest {
    pub sheet: String,
    /// 1-based sheet row of the headers; guessed when absent.
    #[serde(default)]
    pub header_row: Option<usize>,
}

/// What checking a source change against the job came to.
enum SourceCheck {
    Ok(ImportType),
    NotFound,
    BadRequest(String),
    Conflict(String),
}

/// Result variants for the source flow.
enum SourceOutcome {
    Ok(ImportJob),
    NotFound,
    Conflict(String),
}

/// Read a workbook again from another sheet or header row. Allowed while
/// the job is `parsed` or `dry_run_done`. The job returns to `parsed` with
/// the new headers, samples and suggestions; its mapping and check are
/// cleared, since their column numbers belonged to the old grid, and its
/// options are kept.
pub async fn put_source(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
    body: web::Json<SourceRequest>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let SourceRequest { sheet, header_row } = body.into_inner();

    let check = tc.run(|conn| {
        let job = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(SourceCheck::NotFound),
            Err(e) => return Err(e),
        };
        if let Some(message) = source_precondition(&job) {
            return Ok(SourceCheck::Conflict(message));
        }
        let Some(job_type) = ImportType::from_str(&job.job_type) else {
            return Ok(SourceCheck::BadRequest(format!(
                "import job has unknown type '{}'",
                job.job_type
            )));
        };
        match stored_source(&job) {
            Some(stored) if stored.source.format == FileFormat::Xlsx.as_str() => {
                Ok(SourceCheck::Ok(job_type))
            }
            Some(_) => Ok(SourceCheck::BadRequest(
                "this file is a CSV; only a workbook has sheets and header rows to choose"
                    .to_string(),
            )),
            None => Ok(SourceCheck::BadRequest(
                "this import has no file to read; upload the file again".to_string(),
            )),
        }
    });
    let job_type = match check {
        Ok(SourceCheck::Ok(t)) => t,
        Ok(SourceCheck::NotFound) => {
            return errors::not_found_msg(format!("import job {id} not found"))
        }
        Ok(SourceCheck::BadRequest(message)) => return errors::bad_request(message),
        Ok(SourceCheck::Conflict(message)) => return errors::conflict(message),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to load import job");
            return errors::internal("failed to load import job");
        }
    };

    // Read the workbook again and stage its grid beside the current one;
    // it takes the current one's place once the job row agrees.
    let upload = upload_path(id, FileFormat::Xlsx.as_str());
    let choice = SheetChoice { sheet, header_row };
    let read = parse_off_worker(move || -> std::io::Result<Result<_, String>> {
        let bytes = match std::fs::read(&upload) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Err(
                    "this import's file is no longer available; upload it again".to_string(),
                ))
            }
            Err(e) => return Err(e),
        };
        match xlsx::read(&bytes, Some(&choice)) {
            Ok(workbook) => {
                let staged = mapping::StagedGrid::write(id, &workbook.grid)?;
                Ok(Ok((staged, workbook.source)))
            }
            Err(refused) => Ok(Err(refused.0)),
        }
    })
    .await;
    let (staged, source) = match read {
        Ok(Ok(Ok(read))) => read,
        Ok(Ok(Err(message))) => return errors::bad_request(message),
        Ok(Err(e)) => {
            error!(job_id = %id, error = ?e, "failed to store import grid");
            return errors::internal("failed to read import file");
        }
        Err(ParseFailure::TimedOut) => return errors::bad_request(TIMED_OUT),
        Err(ParseFailure::Task(e)) => {
            error!(job_id = %id, error = %e, "import parse task failed");
            return errors::internal("failed to read import file");
        }
    };

    let result = tc.run(|conn| {
        if !repo::try_lock_job(conn, id)? {
            return Ok(SourceOutcome::Conflict(COMMIT_RUNNING.to_string()));
        }
        // Checked again under the transaction: a commit may have finished
        // since the read above.
        let job = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(SourceOutcome::NotFound),
            Err(e) => return Err(e),
        };
        if let Some(message) = source_precondition(&job) {
            return Ok(SourceOutcome::Conflict(message));
        }
        let stored = suggest_for(conn, job_type, source)?;
        let updated = repo::update(
            conn,
            id,
            ImportJobUpdate {
                status: Some("parsed".to_string()),
                source: Some(serde_json::to_value(&stored).ok()),
                mapping: Some(None),
                answers: Some(None),
                summary: Some(None),
                row_results: Some(None),
                ..Default::default()
            },
        )?;
        info!(
            job_id = %id,
            rows = stored.source.row_count,
            "import source chosen"
        );
        Ok(SourceOutcome::Ok(updated))
    });

    match result {
        Ok(SourceOutcome::Ok(job)) => {
            if let Err(e) = staged.install() {
                error!(job_id = %id, error = ?e, "failed to install import grid");
                let message = "The sheet couldn't be saved. Upload the file again.";
                if let Err(mark_err) = tc.run(|conn| {
                    mark_failed(conn, id, message);
                    Ok(())
                }) {
                    error!(job_id = %id, error = ?mark_err, "failed to record import failure");
                }
                return errors::internal(message);
            }
            HttpResponse::Ok().json(ImportJobResponse::from(job))
        }
        Ok(SourceOutcome::NotFound) => errors::not_found_msg(format!("import job {id} not found")),
        Ok(SourceOutcome::Conflict(message)) => errors::conflict(message),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to store import source");
            errors::internal("failed to read import file")
        }
    }
}

/// Why a job's sheet or header row can't change now, if it can't.
fn source_precondition(job: &ImportJob) -> Option<String> {
    match job.status.as_str() {
        "parsed" | "dry_run_done" => None,
        status => Some(format!(
            "job is in status '{status}'; the sheet and header row can change only before the import is committed"
        )),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeRequest {
    #[serde(rename = "type")]
    pub job_type: String,
}

/// Result variants for the type flow.
enum TypeOutcome {
    Ok(ImportJob),
    NotFound,
    BadRequest(String),
    Conflict(String),
}

/// Import the job's file as another type. Allowed while nothing has been
/// committed (`parsed` or `dry_run_done`). The file's columns stay; the
/// suggestions are made again for the new type, and the mapping, options,
/// answers and check are cleared, since they named the old type's fields.
/// The same type answers with the job unchanged.
pub async fn put_type(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
    body: web::Json<TypeRequest>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let job_type = match parse_type(body.job_type.as_str()) {
        Ok(t) => t,
        Err(message) => return errors::bad_request(message),
    };

    let result = tc.run(|conn| {
        if !repo::try_lock_job(conn, id)? {
            return Ok(TypeOutcome::Conflict(COMMIT_RUNNING.to_string()));
        }
        let job = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(TypeOutcome::NotFound),
            Err(e) => return Err(e),
        };
        if let Some(message) = type_precondition(&job) {
            return Ok(TypeOutcome::Conflict(message));
        }
        if job.job_type == job_type.as_str() {
            return Ok(TypeOutcome::Ok(job));
        }
        let Some(stored) = stored_source(&job) else {
            return Ok(TypeOutcome::BadRequest(
                "this import has no file to read; upload the file again".to_string(),
            ));
        };
        let stored = suggest_for(conn, job_type, stored.source)?;
        let updated = repo::update(
            conn,
            id,
            ImportJobUpdate {
                job_type: Some(job_type.as_str().to_string()),
                status: Some("parsed".to_string()),
                source: Some(serde_json::to_value(&stored).ok()),
                mapping: Some(None),
                options: Some(None),
                answers: Some(None),
                summary: Some(None),
                row_results: Some(None),
                ..Default::default()
            },
        )?;
        info!(job_id = %id, kind = job_type.as_str(), "import type changed");
        Ok(TypeOutcome::Ok(updated))
    });

    match result {
        Ok(TypeOutcome::Ok(job)) => HttpResponse::Ok().json(ImportJobResponse::from(job)),
        Ok(TypeOutcome::NotFound) => errors::not_found_msg(format!("import job {id} not found")),
        Ok(TypeOutcome::BadRequest(message)) => errors::bad_request(message),
        Ok(TypeOutcome::Conflict(message)) => errors::conflict(message),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to change import type");
            errors::internal("failed to change the import type")
        }
    }
}

/// Why a job's type can't change now, if it can't.
fn type_precondition(job: &ImportJob) -> Option<String> {
    match job.status.as_str() {
        "parsed" | "dry_run_done" => None,
        status => Some(format!(
            "job is in status '{status}'; the type can change only before the import is committed"
        )),
    }
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
    /// Column indexes set to Don't import. A column neither mapped nor
    /// here is left without a choice; neither imports.
    #[serde(default)]
    pub ignored: Vec<usize>,
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
        ignored,
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
        if let Err(e) = mapping::validate_ignored(&ignored, &column_mapping, &stored.source.headers)
        {
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
    let mut mapped = mapping::map_job_grid(&grid, &column_mapping, &options, &fields);
    let options_json = serde_json::to_value(&options).ok();
    let mut ignored = ignored;
    ignored.sort_unstable();
    let ctx = ImportContext {
        actor: Some(auth.user_uuid),
        job_id: id,
        options,
    };

    let result = tc.run(|conn| {
        if !repo::try_lock_job(conn, id)? {
            return Ok(MappingOutcome::Conflict(COMMIT_RUNNING.to_string()));
        }
        // Checked again under the transaction: a commit may have finished
        // since the read above.
        let job = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(MappingOutcome::NotFound),
            Err(e) => return Err(e),
        };
        if let Some(message) = mapping_precondition(&job) {
            return Ok(MappingOutcome::Conflict(message));
        }
        // The sheet may have been read again since the check above, and the
        // mapping names the columns that were checked.
        let Some(mut current) = stored_source(&job).filter(|s| {
            serde_json::to_value(&s.source).ok() == serde_json::to_value(&stored.source).ok()
        }) else {
            return Ok(MappingOutcome::Conflict(
                "the file was read again while checking; map its columns again".to_string(),
            ));
        };
        current.ignored = ignored.clone();
        // Answers given at Match values stand; those for a field no longer
        // mapped, or that no longer fit, are passed over.
        let mut ctx = ctx;
        values::settle(
            conn,
            job_type,
            &fields,
            &column_mapping,
            &mut ctx.options,
            &stored_answers(&job),
            &mut mapped,
        )?;
        let (summary, rows) = plan(
            conn,
            job_type,
            &mapped,
            &ctx,
            &column_mapping,
            &stored,
            &grid,
        )?;
        let updated = repo::update(
            conn,
            id,
            ImportJobUpdate {
                status: Some("dry_run_done".to_string()),
                summary: Some(serde_json::to_value(&summary).ok()),
                mapping: Some(serde_json::to_value(&column_mapping).ok()),
                source: Some(serde_json::to_value(&current).ok()),
                options: Some(options_json.clone()),
                row_results: Some(stored_rows(RowPhase::Checked, rows)),
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

/// Why a change to a job is refused while its commit runs.
const COMMIT_RUNNING: &str = "this import is being committed; wait for it to finish";

/// The dry run of a mapped grid as the job stores it: the summary and
/// every row's result, errors named by the file's headers and skipped rows
/// with their cells.
fn plan(
    conn: &mut crate::db::DbConnection,
    job_type: ImportType,
    mapped: &mapping::MappedGrid,
    ctx: &ImportContext,
    column_mapping: &ColumnMapping,
    stored: &StoredSource,
    grid: &csv_parser::ParsedCsv,
) -> Result<(imports::ImportSummary, Vec<RowResult>), diesel::result::Error> {
    let (mut summary, mut rows) = imports::check_mapped(conn, job_type, mapped, ctx)?;
    mapping::annotate(&mut summary, column_mapping, &stored.source.headers);
    mapping::complete_rows(&mut rows, column_mapping, &stored.source.headers, grid);
    Ok((summary, rows))
}

/// Whether a fresh plan does what the reviewed one said, row for row: the
/// same outcome on the same record. Wording of errors may differ.
fn same_plan(reviewed: &[RowResult], fresh: &[RowResult]) -> bool {
    reviewed.len() == fresh.len()
        && reviewed
            .iter()
            .zip(fresh)
            .all(|(a, b)| a.row == b.row && a.outcome == b.outcome && a.record_id == b.record_id)
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

/// Settle the options against the workspace and the file: the legacy
/// `match_by: none` becomes create-only by tag, a mode other than create
/// and update is for assets, the default kind must be a kind (stored as its
/// slug), and the decimal mark defaults from the file's delimiter.
fn resolve_options(
    conn: &mut crate::db::DbConnection,
    job_type: ImportType,
    options: &mut ImportOptions,
    source: &ImportSource,
) -> Result<(), String> {
    options.normalize()?;
    if job_type != ImportType::Assets && options.mode != imports::ImportMode::CreateUpdate {
        return Err(format!(
            "mode applies to asset imports only; {} imports {}",
            job_type.as_str(),
            match job_type {
                ImportType::Users => "match people by email",
                _ => "always create",
            }
        ));
    }
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
    /// This job's commit is already running.
    Running,
    /// The job is being changed (its file or mapping).
    Busy(String),
    /// Another import of the same type is committing into the workspace.
    TypeBusy,
    /// The plan differs from the one the admin reviewed; the job now holds
    /// the fresh one.
    PlanChanged(ImportJob),
}

/// Apply a previously dry-run'd job with its stored mapping and options.
/// Idempotent on status: a job already in `done` returns the existing row
/// unchanged.
///
/// The commit takes the job's commit mark, the job's lock (which a change
/// to its file or mapping also takes) and the lock on its type in the
/// workspace, all advisory and transaction-scoped, refusing with 409 rather
/// than waiting when any is held.
/// Under them it plans every row again against the workspace as it is now;
/// when any row would come to something other than what the admin reviewed,
/// nothing is written, the job keeps the fresh plan, and the answer is 409
/// with the job.
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
    let Some(stored) = stored_source(&job) else {
        return errors::bad_request("this import has no file to commit; upload the file again");
    };
    let grid = match read_job_grid(id).await {
        Ok(grid) => grid,
        Err(resp) => return resp,
    };
    let mut parsed = mapping::map_job_grid(&grid, &column_mapping, &options, &fields);
    let mut ctx = ImportContext {
        actor: Some(auth.user_uuid),
        job_id: id,
        options,
    };

    let result = tc.run_result(|conn| -> Result<CommitOutcome, CommitError> {
        if !repo::try_mark_commit(conn, id)? {
            return Ok(CommitOutcome::Running);
        }
        if !repo::try_lock_job(conn, id)? {
            return Ok(CommitOutcome::Busy(
                "the mapping is being changed; check the rows again".to_string(),
            ));
        }
        if !repo::try_lock_type(conn, job.workspace_id, &job.job_type)? {
            return Ok(CommitOutcome::TypeBusy);
        }
        // Read again under the locks: a parallel commit or a new mapping
        // may have landed since the read above.
        let current = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(CommitOutcome::NotFound),
            Err(e) => return Err(e.into()),
        };
        if let Some(outcome) = commit_precondition(&current) {
            return Ok(outcome);
        }
        if current.mapping != job.mapping
            || current.options != job.options
            || current.source != job.source
            || current.answers != job.answers
        {
            return Ok(CommitOutcome::BadRequest(
                "the mapping changed while importing; check the rows again".to_string(),
            ));
        }
        let job_type =
            ImportType::from_str(&current.job_type).expect("checked by commit_precondition");
        values::settle(
            conn,
            job_type,
            &fields,
            &column_mapping,
            &mut ctx.options,
            &stored_answers(&current),
            &mut parsed,
        )?;

        // Plan again against the workspace as it is now and hold it up
        // against what the admin reviewed.
        let reviewed: Option<StoredRowResults> = repo::row_results(conn, id)?
            .and_then(|v| serde_json::from_value(v).ok())
            .filter(|r: &StoredRowResults| r.phase == RowPhase::Checked);
        let (summary, fresh) = plan(
            conn,
            job_type,
            &parsed,
            &ctx,
            &column_mapping,
            &stored,
            &grid,
        )?;
        if !reviewed.is_some_and(|r| same_plan(&r.rows, &fresh)) {
            let updated = repo::update(
                conn,
                id,
                ImportJobUpdate {
                    summary: Some(serde_json::to_value(&summary).ok()),
                    row_results: Some(stored_rows(RowPhase::Checked, fresh)),
                    ..Default::default()
                },
            )?;
            info!(job_id = %id, "import plan changed since the check; nothing written");
            return Ok(CommitOutcome::PlanChanged(updated));
        }

        let committed = imports::apply_mapped(conn, job_type, &parsed, &ctx)?;
        let mut rows = committed.rows;
        mapping::complete_rows(&mut rows, &column_mapping, &stored.source.headers, &grid);
        let record_ids = imports::RecordIds::from_rows(&rows);

        let updated = repo::update(
            conn,
            current.id,
            ImportJobUpdate {
                status: Some("done".to_string()),
                records_committed: Some(Some(committed.records.len() as i32)),
                completed_at: Some(Some(chrono::Utc::now())),
                row_results: Some(stored_rows(RowPhase::Committed, rows)),
                record_ids: Some(serde_json::to_value(&record_ids).ok()),
                ..Default::default()
            },
        )?;
        Ok(CommitOutcome::Ok(updated, committed.records))
    });

    match result {
        Ok(outcome) => commit_response(&search_service, outcome),
        Err(e) => {
            // The commit rolled back every row, and the job's status with
            // it; record the failure on the job in a transaction of its own.
            error!(job_id = %id, error = %e, "commit failed");
            let message = match &e {
                CommitError::Row {
                    row,
                    column,
                    message,
                } => {
                    let header = stored_source(&job).and_then(|s| {
                        mapping::source_header(
                            &column_mapping,
                            &s.source.headers,
                            column.as_deref(),
                        )
                    });
                    match header {
                        Some(header) => format!(
                            "Nothing was imported. Row {row} could not be saved: {message} (column '{header}')."
                        ),
                        None => {
                            format!("Nothing was imported. Row {row} could not be saved: {message}")
                        }
                    }
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

/// Row results as `import_jobs.row_results` stores them.
fn stored_rows(phase: RowPhase, rows: Vec<RowResult>) -> Option<serde_json::Value> {
    serde_json::to_value(StoredRowResults { phase, rows }).ok()
}

/// The mapping and options stored at the Map step; `None` for a job
/// checked before mapping existed.
fn stored_choices(job: &ImportJob) -> Option<(ColumnMapping, ImportOptions)> {
    let column_mapping = serde_json::from_value(job.mapping.clone()?).ok()?;
    let mut options: ImportOptions = match &job.options {
        Some(v) => serde_json::from_value(v.clone()).ok()?,
        None => ImportOptions::default(),
    };
    // A job stored with the legacy `match_by: none` commits as create only.
    options.normalize().ok()?;
    Some((column_mapping, options))
}

/// The answers stored at Match values; none when there are none or they
/// don't read.
fn stored_answers(job: &ImportJob) -> ValueAnswers {
    job.answers
        .clone()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
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
        CommitOutcome::Busy(msg) => errors::conflict(msg),
        CommitOutcome::Running => errors::conflict_with_code(
            "This import is already being committed.",
            "import_commit_running",
        ),
        CommitOutcome::TypeBusy => errors::conflict_with_code(
            "Another import of this type is being committed. Try again when it finishes.",
            "import_type_busy",
        ),
        CommitOutcome::PlanChanged(job) => errors::with_fields(
            actix_web::http::StatusCode::CONFLICT,
            "import_plan_changed",
            "Something changed since you checked. Nothing was imported; review the new plan.",
            serde_json::json!({ "job": ImportJobResponse::from(job) }),
        ),
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
    let read = tc.run(|conn| {
        let mut job = repo::get(conn, id)?;
        // A running commit hasn't written the job yet; say so, so a client
        // whose commit request timed out knows to wait.
        if job.status == "dry_run_done" && repo::commit_running(conn, id)? {
            job.status = "committing".to_string();
        }
        Ok(job)
    });
    match read {
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

/// Result variants for the delete flow.
enum DeleteOutcome {
    Deleted,
    NotFound,
    Conflict(String),
}

/// Discard a job nothing was imported from, and its files: "Change file"
/// on the Map step. A committed job is the record of what it wrote and
/// stays; so does a job whose commit is running.
pub async fn delete_job(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let result = tc.run(|conn| {
        if !repo::try_lock_job(conn, id)? {
            return Ok(DeleteOutcome::Conflict(COMMIT_RUNNING.to_string()));
        }
        let job = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(DeleteOutcome::NotFound),
            Err(e) => return Err(e),
        };
        if job.status == "done" || job.records_committed.is_some() || job.record_ids.is_some() {
            return Ok(DeleteOutcome::Conflict(
                "this import has been committed; only an import nothing was imported from can be deleted"
                    .to_string(),
            ));
        }
        repo::delete(conn, id)?;
        Ok(DeleteOutcome::Deleted)
    });

    match result {
        Ok(DeleteOutcome::Deleted) => {
            // The row is gone, so nothing reads the files again; one left
            // behind is only disk.
            match web::block(move || mapping::remove_job_files(id)).await {
                Ok(Ok(removed)) => info!(job_id = %id, count = removed, "import discarded"),
                Ok(Err(e)) => error!(job_id = %id, error = ?e, "failed to remove import files"),
                Err(e) => error!(job_id = %id, error = ?e, "import file removal task failed"),
            }
            HttpResponse::NoContent().finish()
        }
        Ok(DeleteOutcome::NotFound) => errors::not_found_msg(format!("import job {id} not found")),
        Ok(DeleteOutcome::Conflict(message)) => errors::conflict(message),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to delete import job");
            errors::internal("failed to delete the import")
        }
    }
}

/// A checked job with what Match values reads: its type, the fields, the
/// stored mapping, options and source, and the file's grid mapped.
struct ValuesJob {
    job: ImportJob,
    job_type: ImportType,
    fields: Vec<FieldSpec>,
    column_mapping: ColumnMapping,
    options: ImportOptions,
    stored: StoredSource,
    grid: csv_parser::ParsedCsv,
    mapped: mapping::MappedGrid,
}

/// Why a job's values can't be read or answered now, if they can't.
fn values_precondition(job: &ImportJob) -> Option<String> {
    match job.status.as_str() {
        "dry_run_done" => None,
        "parsed" => Some(NOT_CHECKED.to_string()),
        status => Some(format!(
            "job is in status '{status}'; values can be answered only before the import is committed"
        )),
    }
}

/// Load a checked job for Match values, or the response that refuses it.
async fn load_values_job(tc: &mut TenantConn, id: Uuid) -> Result<ValuesJob, HttpResponse> {
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
            return Err(errors::not_found_msg(format!("import job {id} not found")));
        }
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to load import job");
            return Err(errors::internal("failed to load import job"));
        }
    };
    if let Some(message) = values_precondition(&job) {
        return Err(errors::conflict(message));
    }
    let Some(job_type) = ImportType::from_str(&job.job_type) else {
        return Err(errors::bad_request(format!(
            "import job has unknown type '{}'",
            job.job_type
        )));
    };
    let (Some((column_mapping, options)), Some(stored)) =
        (stored_choices(&job), stored_source(&job))
    else {
        return Err(errors::bad_request(
            "This import was checked before column mapping. Upload the file again.",
        ));
    };
    let grid = read_job_grid(id).await?;
    let mapped = mapping::map_job_grid(&grid, &column_mapping, &options, &fields);
    Ok(ValuesJob {
        job,
        job_type,
        fields,
        column_mapping,
        options,
        stored,
        grid,
        mapped,
    })
}

/// The distinct values of each mapped column Match values asks about, with
/// how each matched, the answer it takes, and the rows the last check
/// skipped.
pub async fn get_values(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let loaded = match load_values_job(&mut tc, id).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let answers = stored_answers(&loaded.job);
    let read = tc.run(|conn| {
        let resolution = values::prepare(
            conn,
            loaded.job_type,
            &loaded.fields,
            &loaded.column_mapping,
            &loaded.options,
            &loaded.mapped.grid,
            &answers,
        )?;
        let checked: Vec<RowResult> = repo::row_results(conn, id)?
            .and_then(|v| serde_json::from_value::<StoredRowResults>(v).ok())
            .filter(|r| r.phase == RowPhase::Checked)
            .map(|r| r.rows)
            .unwrap_or_default();
        Ok(values::view(
            &resolution,
            &loaded.fields,
            &loaded.stored.source.headers,
            &checked,
        ))
    });
    match read {
        Ok(view) => HttpResponse::Ok().json(view),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to read import values");
            errors::internal("failed to read the file's values")
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValuesRequest {
    pub answers: ValueAnswers,
}

/// Result variants for answering values.
enum ValuesOutcome {
    Ok(ImportJob),
    NotFound,
    Conflict(String),
}

/// Store the admin's answers to the file's values, replacing any before,
/// and run the dry run again with them.
pub async fn put_values(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
    body: web::Json<ValuesRequest>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let ValuesRequest { answers } = body.into_inner();
    let ValuesJob {
        job,
        job_type,
        fields,
        column_mapping,
        options,
        stored,
        grid,
        mut mapped,
    } = match load_values_job(&mut tc, id).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if let Err(e) = mapping::validate_answers(&answers, &fields) {
        return errors::bad_request(e.0);
    }
    let checked = tc.run(|conn| {
        let columns = values::matchers(conn, job_type, &fields, &column_mapping, &options)?;
        let blanks = values::blank_fields(job_type, &columns, &options);
        Ok(values::validate(answers, &columns, &blanks))
    });
    let answers = match checked {
        Ok(Ok(answers)) => answers,
        Ok(Err(e)) => return errors::bad_request(e.0),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to check import answers");
            return errors::internal("failed to check the answers");
        }
    };
    let answers_json = serde_json::to_value(&answers).ok();

    let result = tc.run(|conn| {
        if !repo::try_lock_job(conn, id)? {
            return Ok(ValuesOutcome::Conflict(COMMIT_RUNNING.to_string()));
        }
        let current = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(ValuesOutcome::NotFound),
            Err(e) => return Err(e),
        };
        if let Some(message) = values_precondition(&current) {
            return Ok(ValuesOutcome::Conflict(message));
        }
        if current.mapping != job.mapping
            || current.options != job.options
            || current.source != job.source
        {
            return Ok(ValuesOutcome::Conflict(
                "the columns were mapped again while answering; check the values again".to_string(),
            ));
        }
        let mut ctx = ImportContext {
            actor: Some(auth.user_uuid),
            job_id: id,
            options,
        };
        values::settle(
            conn,
            job_type,
            &fields,
            &column_mapping,
            &mut ctx.options,
            &answers,
            &mut mapped,
        )?;
        let (summary, rows) = plan(
            conn,
            job_type,
            &mapped,
            &ctx,
            &column_mapping,
            &stored,
            &grid,
        )?;
        let updated = repo::update(
            conn,
            id,
            ImportJobUpdate {
                answers: Some(answers_json),
                summary: Some(serde_json::to_value(&summary).ok()),
                row_results: Some(stored_rows(RowPhase::Checked, rows)),
                ..Default::default()
            },
        )?;
        info!(
            job_id = %id,
            rows = summary.row_count,
            errors = summary.errors_total,
            "import values answered"
        );
        Ok(ValuesOutcome::Ok(updated))
    });

    match result {
        Ok(ValuesOutcome::Ok(job)) => HttpResponse::Ok().json(ImportJobResponse::from(job)),
        Ok(ValuesOutcome::NotFound) => errors::not_found_msg(format!("import job {id} not found")),
        Ok(ValuesOutcome::Conflict(message)) => errors::conflict(message),
        Err(e) => {
            error!(job_id = %id, error = ?e, "import values check failed");
            errors::internal("failed to check the import")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RowsQuery {
    /// `all` (default), `create`, `update`, `unchanged` or `skipped`
    /// (`error` reads as `skipped`).
    pub filter: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

/// Rows a page carries unless the client asks for fewer.
const ROWS_PAGE: usize = 50;
/// Most rows one page carries.
const ROWS_PAGE_MAX: usize = 200;

/// What reading a job's row results came to.
enum RowResultsRead {
    Ok(ImportJob, StoredRowResults),
    NotFound,
    /// The job hasn't been checked, so it has no row results yet.
    NotChecked,
}

fn read_row_results(
    conn: &mut crate::db::DbConnection,
    id: Uuid,
) -> Result<RowResultsRead, diesel::result::Error> {
    let job = match repo::get(conn, id) {
        Ok(j) => j,
        Err(diesel::result::Error::NotFound) => return Ok(RowResultsRead::NotFound),
        Err(e) => return Err(e),
    };
    let Some(value) = repo::row_results(conn, id)? else {
        return Ok(RowResultsRead::NotChecked);
    };
    match serde_json::from_value(value) {
        Ok(stored) => Ok(RowResultsRead::Ok(job, stored)),
        Err(e) => {
            error!(job_id = %id, error = ?e, "unreadable import row results");
            Ok(RowResultsRead::NotChecked)
        }
    }
}

const NOT_CHECKED: &str = "this import hasn't been checked yet; map its columns first";

/// One page of a job's row results, filtered by outcome, with the count of
/// each outcome. A skipped row's cells stay out; the error report has them.
pub async fn rows(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
    query: web::Query<RowsQuery>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let filter = match query.filter.as_deref().unwrap_or("all") {
        "all" => None,
        "create" => Some(RowOutcome::Create),
        "update" => Some(RowOutcome::Update),
        "unchanged" => Some(RowOutcome::Unchanged),
        "skipped" | "error" => Some(RowOutcome::Skipped),
        other => {
            return errors::bad_request(format!(
                "unknown filter '{other}'; use all, create, update, unchanged or skipped"
            ))
        }
    };
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(ROWS_PAGE).clamp(1, ROWS_PAGE_MAX);

    let stored = match tc.run(|conn| read_row_results(conn, id)) {
        Ok(RowResultsRead::Ok(_, stored)) => stored,
        Ok(RowResultsRead::NotFound) => {
            return errors::not_found_msg(format!("import job {id} not found"))
        }
        Ok(RowResultsRead::NotChecked) => return errors::conflict(NOT_CHECKED),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to load import rows");
            return errors::internal("failed to load import rows");
        }
    };

    let count = |outcome: RowOutcome| stored.rows.iter().filter(|r| r.outcome == outcome).count();
    let counts = serde_json::json!({
        "all": stored.rows.len(),
        "create": count(RowOutcome::Create),
        "update": count(RowOutcome::Update),
        "unchanged": count(RowOutcome::Unchanged),
        "skipped": count(RowOutcome::Skipped),
    });
    let matching: Vec<&RowResult> = stored
        .rows
        .iter()
        .filter(|r| filter.is_none_or(|f| r.outcome == f))
        .collect();
    let page: Vec<RowResult> = matching
        .iter()
        .skip(offset)
        .take(limit)
        .map(|r| RowResult {
            cells: Vec::new(),
            ..(*r).clone()
        })
        .collect();
    HttpResponse::Ok().json(serde_json::json!({
        "phase": stored.phase,
        "total": matching.len(),
        "offset": offset,
        "limit": limit,
        "counts": counts,
        "rows": page,
    }))
}

/// The skipped rows as CSV, in the file's layout with a `Row` and an
/// `Errors` column first (`services::imports::report`). Before the commit
/// these are the rows it would skip; after, the rows it skipped.
pub async fn error_report(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let loaded = tc.run(|conn| {
        let read = read_row_results(conn, id)?;
        let fields = match &read {
            RowResultsRead::Ok(job, _) => match ImportType::from_str(&job.job_type) {
                Some(t) => imports::importer_for(t).fields(conn)?,
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        Ok((read, fields))
    });
    let (job, stored, fields) = match loaded {
        Ok((RowResultsRead::Ok(job, stored), fields)) => (job, stored, fields),
        Ok((RowResultsRead::NotFound, _)) => {
            return errors::not_found_msg(format!("import job {id} not found"))
        }
        Ok((RowResultsRead::NotChecked, _)) => return errors::conflict(NOT_CHECKED),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to load import rows");
            return errors::internal("failed to load import rows");
        }
    };
    let Some(source) = stored_source(&job).map(|s| s.source) else {
        return errors::conflict(NOT_CHECKED);
    };
    let delimiter = match source.delimiter.as_deref() {
        Some(";") => b';',
        Some("\t") => b'\t',
        _ => b',',
    };
    match report::error_report_csv(&source.headers, delimiter, &stored.rows, &fields) {
        Ok(body) => HttpResponse::Ok()
            .insert_header((header::CONTENT_TYPE, "text/csv; charset=utf-8"))
            .insert_header((
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}-errors.csv\"",
                    download_stem(&job.filename)
                ),
            ))
            .body(body),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to write import error report");
            errors::internal("failed to write the error report")
        }
    }
}

/// The uploaded file's name without its extension, reduced to characters
/// safe in a `Content-Disposition` filename.
fn download_stem(filename: &str) -> String {
    let stem = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '_'
            }
        })
        .take(100)
        .collect();
    let safe = safe.trim().to_string();
    if safe.is_empty() {
        "import".to_string()
    } else {
        safe
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

fn persist_upload(
    job_id: Uuid,
    extension: &str,
    bytes: &[u8],
) -> std::io::Result<std::path::PathBuf> {
    let dir = mapping::imports_dir();
    std::fs::create_dir_all(&dir)?;
    let path = upload_path(job_id, extension);
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// Where a job's original upload is kept (`csv` or `xlsx`), derived from
/// the job id alone.
fn upload_path(job_id: Uuid, extension: &str) -> std::path::PathBuf {
    mapping::imports_dir().join(format!("{job_id}.{extension}"))
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
