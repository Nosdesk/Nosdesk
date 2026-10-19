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
//! - `GET  /admin/import?limit=20` lists the newest jobs that reached
//!   Map, light: who, when, the file, the state (Draft, Imported,
//!   Expired) and the outcome counts.
//! - `GET  /admin/import/{id}` returns the job row + summary so
//!   the admin can resume, or read a past import.
//! - `DELETE /admin/import/{id}` discards a job nothing was imported
//!   from, with its files.
//! - `GET  /admin/import/{id}/rows?filter=&offset=&limit=` pages
//!   through every row's result, from the dry run or the commit.
//! - `GET  /admin/import/{id}/error-report` downloads the skipped and
//!   partly imported rows as CSV in the file's own layout, with the
//!   reasons; `?group=`
//!   narrows it to one problem's rows and `?dropped=` lists the rows that
//!   import without some of their values.
//! - `GET  /admin/import/{id}/remove-created` says what Remove imported
//!   assets would do now: the assets the import created, which would be
//!   deleted, and which are kept and why. `POST` does it, once, within 30
//!   days of the commit (`services::imports::remove`).
//! - `GET  /admin/import/{id}/previous-values` downloads, for an asset
//!   import, the values its updates replaced as a file to import back
//!   (`services::imports::previous`); 410 once the details are pruned.
//! - `POST /admin/import/{id}/next-sheet` starts a new draft on another
//!   sheet of a committed import's workbook, with nothing carried but the
//!   file (`services::imports::next_sheet`).
//! - `POST /admin/import/{id}/invites` sends an invitation to each person a
//!   committed users import created who can't sign in yet
//!   (`services::imports::sign_in`).
//! - `GET  /admin/import/fields?type=` lists the fields a column
//!   can map to.
//! - `GET  /admin/import/answers?type=` lists the type's saved matches
//!   and the header memory of its latest mapped import;
//!   `DELETE /admin/import/answers?type=` deletes every saved match of
//!   the type and `DELETE /admin/import/answers/{id}` one of them.
//! - `GET  /admin/import/template/{type}?kind=` returns a CSV header
//!   row the admin can fill in; for assets, with the kind's attributes.
//!
//! - `POST /admin/import?type=documentation` and the
//!   `/admin/import/{id}/docs/...` routes import documentation from a zip of
//!   Markdown (`handlers::imports_docs`).
//!
//! Every route is workspace-admin only, and files are read before any
//! transaction opens.
//!
//! Thirty days after its commit, or after a draft's last change, the prune
//! drops a job's file and per-row detail (`services::imports::history`):
//! `rows` and `error-report` then answer 410, and a draft can't go on.

use actix_multipart::Multipart;
use actix_web::{http::header, web, HttpRequest, HttpResponse, Responder};
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

use std::sync::Arc;

use crate::errors;
use crate::extractors::{AuthContext, ScopedStorage, TenantConn};
use crate::models::{
    ImportCounts, ImportJob, ImportJobListItem, ImportJobResponse, ImportJobUpdate, NewImportJob,
    RemovedCounts,
};
use crate::repository::assets::AssetDeletedObserver;
use crate::repository::imports as repo;
use crate::services::imports::documentation;
use crate::services::imports::files::{self, FileError};
use crate::services::imports::history::{self, JobState};
use crate::services::imports::mapping::{self, ColumnMapping, ImportSource, StoredSource};
use crate::services::imports::next_sheet;
use crate::services::imports::previous;
use crate::services::imports::refusal::Refused;
use crate::services::imports::remove;
use crate::services::imports::saved;
use crate::services::imports::sign_in;
use crate::services::imports::values;
use crate::services::imports::xlsx::{self, FileFormat, SheetChoice};
use crate::services::imports::{
    self, csv_parser, report, CommitError, FieldSpec, ImportContext, ImportOptions, ImportType,
    ImportedRecords, RowOutcome, RowPhase, RowResult, StoredRowResults, ValueAnswers,
};
use crate::services::search::{indexing_tasks, SearchService};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/admin/import")
            .route(web::get().to(crate::handlers::imports::list_jobs))
            .route(web::post().to(crate::handlers::imports::upload)),
    )
    // Before `/admin/import/{id}`, which would take "fields" or
    // "answers" as an id.
    .route(
        "/admin/import/fields",
        web::get().to(crate::handlers::imports::fields),
    )
    .route(
        "/admin/import/template/{type}",
        web::get().to(crate::handlers::imports::template),
    )
    .service(
        web::resource("/admin/import/answers")
            .route(web::get().to(crate::handlers::imports::list_answers))
            .route(web::delete().to(crate::handlers::imports::delete_answers)),
    )
    .route(
        "/admin/import/answers/{id}",
        web::delete().to(crate::handlers::imports::delete_answer),
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
        "/admin/import/{id}/values/preview",
        web::post().to(crate::handlers::imports::preview_values),
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
        web::resource("/admin/import/{id}/remove-created")
            .route(web::get().to(crate::handlers::imports::removal_preview))
            .route(web::post().to(crate::handlers::imports::remove_created)),
    )
    .route(
        "/admin/import/{id}/previous-values",
        web::get().to(crate::handlers::imports::previous_values),
    )
    .route(
        "/admin/import/{id}/next-sheet",
        web::post().to(crate::handlers::imports::next_sheet),
    )
    .route(
        "/admin/import/{id}/invites",
        web::post().to(crate::handlers::imports::send_invites),
    )
    .configure(crate::handlers::imports_docs::config)
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
    /// The file can't be imported.
    Refused(Refused),
}

/// A file that can't be imported: 400 with the refusal's code, its English
/// and the values the client words it with.
pub(crate) fn refused_response(refused: Refused) -> HttpResponse {
    errors::with_fields(
        actix_web::http::StatusCode::BAD_REQUEST,
        refused.code,
        refused.message,
        serde_json::json!({ "args": refused.args }),
    )
}

/// Upload + parse: store the file and its grid, describe its columns and
/// suggest a mapping. The job waits in `parsed` for the mapping. A file
/// that can't be imported is refused with no job and its upload removed.
pub async fn upload(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    query: web::Query<UploadQuery>,
    req: HttpRequest,
    mut payload: Multipart,
) -> impl Responder {
    // Bulk import can create/update users (including their roles),
    // assets, and tickets. Restrict to workspace admins. See
    // security-audit-2026-06.
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let user_uuid = auth.user_uuid;

    // A documentation import streams a zip of Markdown; everything else
    // here reads a spreadsheet.
    if query.job_type == crate::services::imports::documentation::JOB_TYPE {
        return crate::handlers::imports_docs::upload(tc, auth, storage, req, payload).await;
    }

    let job_type = match parse_type(query.job_type.as_str()) {
        Ok(t) => t,
        Err(message) => return errors::bad_request(message),
    };

    // Pull the first file field. Like the branding upload we
    // only consume the first multipart field; extra fields are
    // ignored.
    let (filename, file_bytes) = match read_first_file_field(&mut payload).await {
        Ok(v) => v,
        Err(refused) => return refused_response(refused),
    };
    if file_bytes.is_empty() {
        return refused_response(Refused::new("import_file_empty", "uploaded file is empty"));
    }

    // A workbook by its first bytes; anything else reads as CSV.
    let format = xlsx::detect(&file_bytes);

    // The raw upload and its grid are both kept under the job id. A v7 id
    // dates the files, so the sweep for files without a job can leave an
    // upload alone while its job is still being created.
    let job_id = Uuid::now_v7();
    let extension = format.as_ref().map_or("bin", |f| f.as_str());
    let stored_key =
        match files::put_upload(storage.0.as_ref(), job_id, extension, &file_bytes).await {
            Ok(key) => key,
            Err(e) => {
                error!(error = ?e, "failed to store import upload");
                return errors::internal("failed to save uploaded file");
            }
        };

    // Parse off the async worker and before any transaction opens.
    let read = parse_off_worker(move || -> std::io::Result<(ReadOutcome, Vec<u8>)> {
        let format = match format {
            Ok(f) => f,
            Err(refused) => return Ok((ReadOutcome::Refused(refused), Vec::new())),
        };
        match format {
            FileFormat::Xlsx => match xlsx::read(&file_bytes, None) {
                Ok(workbook) => {
                    let grid = files::grid_bytes(&workbook.grid)?;
                    Ok((ReadOutcome::Read(workbook.source), grid))
                }
                Err(refused) => Ok((ReadOutcome::Refused(refused), Vec::new())),
            },
            FileFormat::Csv => {
                let parsed = match csv_parser::parse_bytes(&file_bytes) {
                    Ok(p) => p,
                    Err(e) => return Ok((ReadOutcome::Refused(e.refusal()), Vec::new())),
                };
                if parsed.rows.is_empty() {
                    return Ok((
                        ReadOutcome::Refused(Refused::new(
                            "import_no_rows",
                            "The file has a header row and no data.",
                        )),
                        Vec::new(),
                    ));
                }
                let grid = files::grid_bytes(&parsed)?;
                Ok((ReadOutcome::Read(mapping::csv_source(&parsed)), grid))
            }
        }
    })
    .await;
    let (read, grid_bytes) = match read {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(e)) => {
            error!(error = ?e, "failed to encode import grid");
            return errors::internal("failed to save uploaded file");
        }
        Err(ParseFailure::TimedOut) => (ReadOutcome::Refused(timed_out()), Vec::new()),
        Err(ParseFailure::Task(e)) => {
            error!(error = %e, "import parse task failed");
            return errors::internal("failed to read uploaded file");
        }
    };
    let source = match read {
        ReadOutcome::Read(source) => source,
        ReadOutcome::Refused(refused) => {
            // No job names the upload.
            remove_files(&storage, job_id).await;
            return refused_response(refused);
        }
    };
    let grid = match files::put_grid(storage.0.as_ref(), job_id, &grid_bytes).await {
        Ok(version) => version,
        Err(e) => {
            error!(error = ?e, "failed to store import grid");
            remove_files(&storage, job_id).await;
            return errors::internal("failed to save uploaded file");
        }
    };
    drop(grid_bytes);

    let job_type_str = job_type.as_str().to_string();
    let result = tc.run(|conn| {
        let job = repo::create(
            conn,
            NewImportJob {
                id: job_id,
                job_type: job_type_str.clone(),
                filename: stored_filename(&filename),
                file_path: stored_key,
                created_by: Some(user_uuid),
                upload_of: None,
            },
        )?;

        let stored = suggest_for(conn, job_type, source, Some(grid))?;

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
        Ok(updated)
    });

    match result {
        Ok(job) => HttpResponse::Ok().json(ImportJobResponse::from(job)),
        Err(e) => {
            error!(error = ?e, "import upload failed");
            // No job names these files.
            remove_files(&storage, job_id).await;
            errors::internal("failed to start import")
        }
    }
}

/// Longest a file may take to parse.
const PARSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn timed_out() -> Refused {
    Refused::new(
        "import_parse_timed_out",
        "Reading this file took too long. Copy the sheet you need into a new file and upload that.",
    )
}

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
/// and synonyms. `grid` is the version of the grid the source was read into.
fn suggest_for(
    conn: &mut crate::db::DbConnection,
    job_type: ImportType,
    source: ImportSource,
    grid: Option<Uuid>,
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
        grid,
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
    /// Another import from the same upload brought the sheet in.
    AlreadyImported,
}

/// Result variants for the source flow.
enum SourceOutcome {
    /// The job, and the grid version its source named before.
    Ok(ImportJob, Option<Uuid>),
    NotFound,
    Conflict(String),
    AlreadyImported,
}

/// Whether a committed import made from the same upload as `job` (the
/// first job it was sent to, or a next sheet) already read `sheet`.
fn sheet_already_imported(
    conn: &mut crate::db::DbConnection,
    job: &ImportJob,
    sheet: &str,
) -> Result<bool, diesel::result::Error> {
    let root = next_sheet::root_of(job.id, job.upload_of);
    let jobs = repo::jobs_of_upload(conn, root)?;
    Ok(next_sheet::imported_sheets(&jobs).contains(sheet))
}

/// Read a workbook again from another sheet or header row. Allowed while
/// the job is `parsed` or `dry_run_done`. The job returns to `parsed` with
/// the new headers, samples and suggestions; its mapping and check are
/// cleared, since their column numbers belonged to the old grid, and its
/// options are kept.
pub async fn put_source(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
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
        if sheet_already_imported(conn, &job, &sheet)? {
            return Ok(SourceCheck::AlreadyImported);
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
        Ok(SourceCheck::AlreadyImported) => {
            return next_sheet_refused(next_sheet::Refusal::AlreadyImported)
        }
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to load import job");
            return errors::internal("failed to load import job");
        }
    };

    // Read the workbook again and store its grid as a new version beside
    // the current one; the job switches to it once the row agrees.
    let bytes = match files::read_upload(storage.0.as_ref(), id, FileFormat::Xlsx.as_str()).await {
        Ok(b) => b,
        Err(e) => return file_error(id, e),
    };
    let wanted = sheet.clone();
    let choice = SheetChoice { sheet, header_row };
    let read = parse_off_worker(move || -> std::io::Result<Result<_, Refused>> {
        match xlsx::read(&bytes, Some(&choice)) {
            Ok(workbook) => Ok(Ok((files::grid_bytes(&workbook.grid)?, workbook.source))),
            Err(refused) => Ok(Err(refused)),
        }
    })
    .await;
    let (grid_bytes, source) = match read {
        Ok(Ok(Ok(read))) => read,
        Ok(Ok(Err(refused))) => return refused_response(refused),
        Ok(Err(e)) => {
            error!(job_id = %id, error = ?e, "failed to encode import grid");
            return errors::internal("failed to read import file");
        }
        Err(ParseFailure::TimedOut) => return refused_response(timed_out()),
        Err(ParseFailure::Task(e)) => {
            error!(job_id = %id, error = %e, "import parse task failed");
            return errors::internal("failed to read import file");
        }
    };
    let new_grid = match files::put_grid(storage.0.as_ref(), id, &grid_bytes).await {
        Ok(version) => version,
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to store import grid");
            return errors::internal("failed to read import file");
        }
    };
    drop(grid_bytes);

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
        // A next sheet from the upload may have committed since.
        if sheet_already_imported(conn, &job, &wanted)? {
            return Ok(SourceOutcome::AlreadyImported);
        }
        let replaced = stored_source(&job).and_then(|s| s.grid);
        let stored = suggest_for(conn, job_type, source, Some(new_grid))?;
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
        Ok(SourceOutcome::Ok(updated, replaced))
    });

    // The version the job doesn't name is only storage now.
    let unused = match &result {
        Ok(SourceOutcome::Ok(_, replaced)) => replaced.filter(|v| *v != new_grid),
        _ => Some(new_grid),
    };
    if let Some(version) = unused {
        if let Err(e) = files::remove_grid(storage.0.as_ref(), id, version).await {
            error!(job_id = %id, error = ?e, "failed to remove an unused import grid");
        }
    }

    match result {
        Ok(SourceOutcome::Ok(job, _)) => HttpResponse::Ok().json(ImportJobResponse::from(job)),
        Ok(SourceOutcome::NotFound) => errors::not_found_msg(format!("import job {id} not found")),
        Ok(SourceOutcome::Conflict(message)) => errors::conflict(message),
        Ok(SourceOutcome::AlreadyImported) => {
            next_sheet_refused(next_sheet::Refusal::AlreadyImported)
        }
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to store import source");
            errors::internal("failed to read import file")
        }
    }
}

/// Why a job's sheet or header row can't change now, if it can't.
fn source_precondition(job: &ImportJob) -> Option<String> {
    if expired(job) {
        return Some(EXPIRED.to_string());
    }
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
        let stored = suggest_for(conn, job_type, stored.source, stored.grid)?;
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
    if job.job_type == documentation::JOB_TYPE {
        return Some("a documentation import can't be read as another type".to_string());
    }
    if expired(job) {
        return Some(EXPIRED.to_string());
    }
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

/// The saved matches of an import type, with the header memory of its
/// latest mapped import, for the Saved matches page.
pub async fn list_answers(
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
    let listed = tc.run(|conn| {
        let fields = imports::importer_for(job_type).fields(conn)?;
        saved::list(conn, job_type, &fields)
    });
    match listed {
        Ok(view) => HttpResponse::Ok().json(view),
        Err(e) => {
            error!(error = ?e, "failed to list saved matches");
            errors::internal("failed to list saved matches")
        }
    }
}

/// Delete one saved match; the next import asks about its value again.
pub async fn delete_answer(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    match tc.run(|conn| repo::delete_value_answer(conn, id)) {
        Ok(0) => errors::not_found_msg("saved match not found"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(entity_id = %id, error = ?e, "failed to delete a saved match");
            errors::internal("failed to delete the saved match")
        }
    }
}

/// Delete every saved match of an import type. The header memory stays.
pub async fn delete_answers(
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
    match tc.run(|conn| repo::delete_value_answers(conn, job_type.as_str())) {
        Ok(deleted) => HttpResponse::Ok().json(serde_json::json!({ "deleted": deleted })),
        Err(e) => {
            error!(error = ?e, "failed to delete saved matches");
            errors::internal("failed to delete the saved matches")
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
    storage: ScopedStorage,
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

    let grid = match read_job_grid(&storage, id, stored.grid).await {
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
            s.grid == stored.grid
                && serde_json::to_value(&s.source).ok() == serde_json::to_value(&stored.source).ok()
        }) else {
            return Ok(MappingOutcome::Conflict(
                "the file was read again while checking; map its columns again".to_string(),
            ));
        };
        current.ignored = ignored.clone();
        // Answers given at Match values stand; those for a field no longer
        // mapped, or that no longer fit, are passed over.
        let mut ctx = ctx;
        let resolution = values::settle(
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
            &resolution,
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
/// with their cells. The summary says where each problem is fixed, which
/// changes a constant writes, and which columns no field takes.
#[allow(clippy::too_many_arguments)]
fn plan(
    conn: &mut crate::db::DbConnection,
    job_type: ImportType,
    mapped: &mapping::MappedGrid,
    resolution: &values::Resolution,
    ctx: &ImportContext,
    column_mapping: &ColumnMapping,
    stored: &StoredSource,
    grid: &csv_parser::ParsedCsv,
) -> Result<(imports::ImportSummary, Vec<RowResult>), diesel::result::Error> {
    let (mut summary, mut rows) = imports::check_mapped(conn, job_type, mapped, ctx)?;
    mapping::annotate(&mut summary, column_mapping, &stored.source.headers);
    mapping::complete_rows(&mut rows, column_mapping, &stored.source.headers, grid);
    values::classify_fixes(&mut summary, &rows, resolution, column_mapping);
    for change in &mut summary.changes {
        change.constant = ctx
            .options
            .constants
            .iter()
            .any(|c| c.field == change.field && c.apply_to == imports::ApplyTo::All);
    }
    summary.unmapped = mapping::unmapped_columns(column_mapping, &stored.source.headers, grid);
    Ok((summary, rows))
}

/// Whether a fresh plan does what the reviewed one said, row for row: the
/// same outcome on the same record and kind, writing the same fields from
/// the same values to the same values. Wording of errors may differ.
fn same_plan(reviewed: &[RowResult], fresh: &[RowResult]) -> bool {
    reviewed.len() == fresh.len()
        && reviewed.iter().zip(fresh).all(|(a, b)| {
            a.row == b.row
                && a.outcome == b.outcome
                && a.record_id == b.record_id
                && a.kind == b.kind
                && a.fields_written == b.fields_written
        })
}

/// Why a job's mapping can't change now, if it can't.
fn mapping_precondition(job: &ImportJob) -> Option<String> {
    if expired(job) {
        return Some(EXPIRED.to_string());
    }
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

/// Read the grid version a job's source names from storage.
async fn read_job_grid(
    storage: &ScopedStorage,
    id: Uuid,
    version: Option<Uuid>,
) -> Result<csv_parser::ParsedCsv, HttpResponse> {
    files::read_grid(storage.0.as_ref(), id, version)
        .await
        .map_err(|e| file_error(id, e))
}

/// The answer to a job file that couldn't be read: 409
/// `import_file_missing` when it is gone, so the admin uploads it again.
fn file_error(id: Uuid, e: FileError) -> HttpResponse {
    match e {
        FileError::Missing => {
            info!(job_id = %id, "import file missing");
            errors::conflict_with_code(files::MISSING, files::MISSING_CODE)
        }
        e => {
            error!(job_id = %id, error = ?e, "failed to read import file");
            errors::internal("failed to read import file")
        }
    }
}

/// Remove a job's files, logging a failure: the sweep catches what's left.
async fn remove_files(storage: &ScopedStorage, id: Uuid) -> usize {
    match files::remove_job_files(storage.0.as_ref(), id).await {
        Ok(removed) => removed,
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to remove import files");
            0
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
    storage: ScopedStorage,
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
    let grid = match read_job_grid(&storage, id, stored.grid).await {
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
        let resolution = values::settle(
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
            &resolution,
            &ctx,
            &column_mapping,
            &stored,
            &grid,
        )?;
        // The catalog entries it creates are part of the plan too, and so
        // are the values rows import without (a group left off, a person
        // left unassigned), which an answer that no longer fits changes.
        let reviewed_summary = current
            .summary
            .as_ref()
            .and_then(|s| serde_json::from_value::<imports::ImportSummary>(s.clone()).ok())
            .unwrap_or_default();
        if !reviewed.is_some_and(|r| same_plan(&r.rows, &fresh))
            || reviewed_summary.catalog != summary.catalog
            || reviewed_summary.dropped != summary.dropped
        {
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
        // What the admin answered becomes the next import's saved match.
        let answers = saved::answers_to_save(&resolution, &committed.created);
        saved::save(conn, job_type, answers, ctx.actor, current.id)?;

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
        Ok(CommitOutcome::Ok(job, records)) => {
            index_imported_records(search_service.get_ref(), records);
            // Done renders from this answer: the same page as a reload.
            match tc.run(|conn| page_response(conn, job.clone())) {
                Ok(response) => HttpResponse::Ok().json(response),
                Err(e) => {
                    error!(job_id = %id, error = ?e, "failed to read a committed import's page");
                    HttpResponse::Ok().json(ImportJobResponse::from(job))
                }
            }
        }
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
        Some(v) => ImportOptions::from_stored(v.clone()).ok()?,
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
    if expired(job) {
        return Some(CommitOutcome::BadRequest(EXPIRED.to_string()));
    }
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

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<i64>,
}

/// Recent imports: the newest jobs that reached Map, at most `limit`
/// (20 by default, up to 50).
pub async fn list_jobs(
    mut tc: TenantConn,
    auth: AuthContext,
    query: web::Query<ListQuery>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let limit = query.limit.unwrap_or(20).clamp(1, 50);
    let read = tc.run(|conn| {
        let rows = repo::list_reached_map(conn, limit)?;
        let mut uuids: Vec<Uuid> = rows.iter().filter_map(|r| r.created_by).collect();
        uuids.sort_unstable();
        uuids.dedup();
        let names = names_of(conn, uuids)?;
        Ok((rows, names))
    });
    let (rows, names) = match read {
        Ok(v) => v,
        Err(e) => {
            error!(error = ?e, "failed to list import jobs");
            return errors::internal("failed to list imports");
        }
    };
    let now = chrono::Utc::now();
    let jobs: Vec<ImportJobListItem> = rows
        .into_iter()
        .map(|r| {
            let checked = r.would_create.is_some();
            ImportJobListItem {
                state: history::state_of(&r.status, r.updated_at, r.pruned_at, now),
                counts: checked.then(|| ImportCounts {
                    new: r.would_create.unwrap_or(0),
                    updated: r.would_update.unwrap_or(0),
                    unchanged: r.unchanged.unwrap_or(0),
                    skipped: r.skipped.unwrap_or(0),
                }),
                created_by_name: r.created_by.and_then(|u| names.get(&u).cloned()),
                id: r.id,
                job_type: r.job_type,
                status: r.status,
                filename: r.filename,
                sheet: r.sheet,
                created_by: r.created_by,
                created_at: r.created_at,
                updated_at: r.updated_at,
                completed_at: r.completed_at,
                pruned_at: r.pruned_at,
                removed: r
                    .removed
                    .zip(r.removed_of)
                    .map(|(deleted, created)| RemovedCounts { deleted, created }),
            }
        })
        .collect();
    HttpResponse::Ok().json(serde_json::json!({ "jobs": jobs }))
}

/// The names the workspace knows these people by.
fn names_of(
    conn: &mut crate::db::DbConnection,
    uuids: Vec<Uuid>,
) -> Result<std::collections::HashMap<Uuid, String>, diesel::result::Error> {
    if uuids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    Ok(
        crate::repository::users::get_user_map_by_uuids_with_persona(&uuids, conn)?
            .into_iter()
            .map(|(uuid, user)| (uuid, user.name))
            .collect(),
    )
}

pub async fn get_job(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    path: web::Path<Uuid>,
) -> impl Responder {
    // Job rows carry import previews that can include other users'
    // PII. Workspace-admin only. See security-audit-2026-06.
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    // A documentation import's background run that died is put back first.
    let settled = tc.run(|conn| {
        let job = repo::get(conn, id)?;
        if documentation::reconcile::stalled(&job, chrono::Utc::now()) {
            return documentation::reconcile::settle(conn, id);
        }
        Ok(Vec::new())
    });
    match settled {
        Ok(keys) => documentation::reconcile::remove_stored(storage.get(), keys).await,
        Err(diesel::result::Error::NotFound) => {
            return errors::not_found_msg(format!("import job {id} not found"));
        }
        Err(e) => {
            error!(error = ?e, "failed to load import job");
            return errors::internal("failed to load import job");
        }
    }
    let read = tc.run(|conn| {
        let mut job = repo::get(conn, id)?;
        // A running commit hasn't written the job yet; say so, so a client
        // whose commit request timed out knows to wait.
        if job.status == "dry_run_done" && repo::commit_running(conn, id)? {
            job.status = "committing".to_string();
        }
        page_response(conn, job)
    });
    match read {
        Ok(response) => HttpResponse::Ok().json(response),
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
    storage: ScopedStorage,
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
            // behind is only storage, which the sweep removes.
            let removed = remove_files(&storage, id).await;
            info!(job_id = %id, count = removed, "import discarded");
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
    if expired(job) {
        return Some(EXPIRED.to_string());
    }
    match job.status.as_str() {
        "dry_run_done" => None,
        "parsed" => Some(NOT_CHECKED.to_string()),
        status => Some(format!(
            "job is in status '{status}'; values can be answered only before the import is committed"
        )),
    }
}

/// Load a checked job for Match values, or the response that refuses it.
async fn load_values_job(
    tc: &mut TenantConn,
    storage: &ScopedStorage,
    id: Uuid,
) -> Result<ValuesJob, HttpResponse> {
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
    let grid = read_job_grid(storage, id, stored.grid).await?;
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
    storage: ScopedStorage,
    path: web::Path<Uuid>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let loaded = match load_values_job(&mut tc, &storage, id).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let answers = stored_answers(&loaded.job);
    read_values(&mut tc, id, &loaded, &answers)
}

/// The values view of a loaded job under `answers`, against the rows of
/// its last check.
fn read_values(
    tc: &mut TenantConn,
    id: Uuid,
    loaded: &ValuesJob,
    answers: &ValueAnswers,
) -> HttpResponse {
    let read = tc.run(|conn| {
        let resolution = values::prepare(
            conn,
            loaded.job_type,
            &loaded.fields,
            &loaded.column_mapping,
            &loaded.options,
            &loaded.mapped.grid,
            answers,
        )?;
        let checked: Vec<RowResult> = repo::row_results(conn, id)?
            .and_then(|v| serde_json::from_value::<StoredRowResults>(v).ok())
            .filter(|r| r.phase == RowPhase::Checked)
            .map(|r| r.rows)
            .unwrap_or_default();
        Ok(values::view(
            &resolution,
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

/// The values view as `answers` would make it, without storing them or
/// checking the rows: models are matched under the manufacturer answers
/// given, so Match values can show the Model card for answers not yet
/// checked. Answers that don't fit fall back as the stored ones do.
pub async fn preview_values(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    path: web::Path<Uuid>,
    body: web::Json<ValuesRequest>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let ValuesRequest { answers } = body.into_inner();
    let loaded = match load_values_job(&mut tc, &storage, id).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if let Err(e) = mapping::validate_answers(&answers, &loaded.fields) {
        return errors::bad_request(e.0);
    }
    let answers = rekey_answers(answers);
    read_values(&mut tc, id, &loaded, &answers)
}

/// Answers keyed by each value as [`values::normalize_value`] gives it.
fn rekey_answers(answers: ValueAnswers) -> ValueAnswers {
    answers
        .into_iter()
        .map(|(field, contexts)| {
            let contexts = contexts
                .into_iter()
                .map(|(context, values)| {
                    let values = values
                        .into_iter()
                        .map(|(value, answer)| (values::normalize_value(&value), answer))
                        .collect();
                    (context, values)
                })
                .collect();
            (field, contexts)
        })
        .collect()
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
    storage: ScopedStorage,
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
    } = match load_values_job(&mut tc, &storage, id).await {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    if let Err(e) = mapping::validate_answers(&answers, &fields) {
        return errors::bad_request(e.0);
    }
    let checked = tc.run(|conn| {
        let columns = values::matchers_for_answers(
            conn,
            job_type,
            &fields,
            &column_mapping,
            &options,
            &answers,
        )?;
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
        let resolution = values::settle(
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
            &resolution,
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
    /// The prune dropped the job's row results.
    Pruned,
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
    if job.pruned_at.is_some() {
        return Ok(RowResultsRead::Pruned);
    }
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
        Ok(RowResultsRead::Pruned) => return details_pruned(),
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

#[derive(Debug, Deserialize)]
pub struct ErrorReportQuery {
    /// Only the rows of one of the summary's error groups, by its index.
    pub group: Option<usize>,
    /// Only the rows that import without some of their values, for the
    /// summary's dropped groups at these indexes (comma-separated).
    pub dropped: Option<String>,
}

/// Which rows a report lists.
enum ReportRows {
    /// Every skipped row and every row that imports without some of its
    /// values.
    Skipped,
    /// The skipped rows with this error.
    Group(imports::ErrorGroup),
    /// The rows that leave out a value of one of these groups.
    Dropped(Vec<imports::DroppedGroup>),
}

/// The error report as CSV, in the file's layout with a `Row` and an
/// `Errors` column first (`services::imports::report`): the skipped rows
/// and the partly imported ones, those that import without some of their
/// values. A value is left out when the row's kind has no such attribute,
/// a group is unknown or answered Leave off, or a person is not a member or
/// answered Leave blank (`RowResult::dropped`); a Leave blank answer on any
/// other column clears the cell before the check, so it is not counted.
/// Before the commit the rows are the plan's; after, what it wrote. `group`
/// narrows it to the skipped rows of one problem; `dropped` to the rows
/// that leave out a value of those groups.
pub async fn error_report(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    path: web::Path<Uuid>,
    query: web::Query<ErrorReportQuery>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let dropped_indexes: Option<Vec<usize>> = match query.dropped.as_deref() {
        None => None,
        Some(raw) => match raw
            .split(',')
            .map(|i| i.trim().parse::<usize>())
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(list) if !list.is_empty() => Some(list),
            _ => return errors::bad_request("dropped takes indexes separated by commas"),
        },
    };
    if query.group.is_some() && dropped_indexes.is_some() {
        return errors::bad_request("ask for one of group and dropped, not both");
    }
    let loaded = tc.run(|conn| {
        let read = read_row_results(conn, id)?;
        let (fields, kinds) = match &read {
            RowResultsRead::Ok(job, _) => match ImportType::from_str(&job.job_type) {
                Some(t) => {
                    let kinds = if t == ImportType::Assets {
                        crate::repository::asset_kinds::list_kinds(conn)?
                            .into_iter()
                            .map(|k| (k.slug, k.label))
                            .collect()
                    } else {
                        std::collections::HashMap::new()
                    };
                    (imports::importer_for(t).fields(conn)?, kinds)
                }
                None => (Vec::new(), std::collections::HashMap::new()),
            },
            _ => (Vec::new(), std::collections::HashMap::new()),
        };
        Ok((read, fields, kinds))
    });
    let (job, stored, fields, kinds) = match loaded {
        Ok((RowResultsRead::Ok(job, stored), fields, kinds)) => (job, stored, fields, kinds),
        Ok((RowResultsRead::NotFound, _, _)) => {
            return errors::not_found_msg(format!("import job {id} not found"))
        }
        Ok((RowResultsRead::NotChecked, _, _)) => return errors::conflict(NOT_CHECKED),
        Ok((RowResultsRead::Pruned, _, _)) => return details_pruned(),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to load import rows");
            return errors::internal("failed to load import rows");
        }
    };
    let Some((source, grid_version)) = stored_source(&job).map(|s| (s.source, s.grid)) else {
        return errors::conflict(NOT_CHECKED);
    };
    let summary: imports::ImportSummary = job
        .summary
        .clone()
        .and_then(|s| serde_json::from_value(s).ok())
        .unwrap_or_default();
    let wanted = match (query.group, dropped_indexes) {
        (Some(i), _) => match summary.error_groups.get(i) {
            Some(group) => ReportRows::Group(group.clone()),
            None => return errors::bad_request(format!("this import has no problem {i}")),
        },
        (None, Some(list)) => {
            let mut groups = Vec::with_capacity(list.len());
            for i in list {
                match summary.dropped.get(i) {
                    Some(group) => groups.push(group.clone()),
                    None => {
                        return errors::bad_request(format!(
                            "this import has no left-out values {i}"
                        ))
                    }
                }
            }
            ReportRows::Dropped(groups)
        }
        (None, None) => ReportRows::Skipped,
    };
    let delimiter = match source.delimiter.as_deref() {
        Some(";") => b';',
        Some("\t") => b'\t',
        _ => b',',
    };

    let body = match &wanted {
        ReportRows::Skipped => {
            // A row that imports keeps no cells; the file has them.
            let grid = if report::has_partial_rows(&stored.rows) {
                match read_job_grid(&storage, id, grid_version).await {
                    Ok(grid) => Some(grid),
                    Err(resp) => return resp,
                }
            } else {
                None
            };
            let cells: std::collections::HashMap<usize, &Vec<String>> = grid
                .iter()
                .flat_map(|g| g.rows.iter().map(|r| (r.source_row, &r.cells)))
                .collect();
            let kind_label =
                |slug: &str| kinds.get(slug).cloned().unwrap_or_else(|| slug.to_string());
            report::error_report_csv(
                &source.headers,
                delimiter,
                &stored.rows,
                &fields,
                stored.phase,
                kind_label,
                |row| cells.get(&row).map_or(&[][..], |c| c.as_slice()),
            )
        }
        ReportRows::Group(group) => {
            let lines = stored
                .rows
                .iter()
                .filter(|r| r.outcome == RowOutcome::Skipped)
                .filter(|r| {
                    r.errors
                        .iter()
                        .any(|e| e.column == group.column && e.group_message() == group.message)
                })
                .map(|r| report::ReportLine {
                    row: r.row,
                    errors: report::errors_text(&r.errors, &fields),
                    cells: &r.cells,
                });
            report::report_csv(&source.headers, delimiter, lines)
        }
        ReportRows::Dropped(groups) => {
            // Rows that import keep no cells; the file has them.
            let grid = match read_job_grid(&storage, id, grid_version).await {
                Ok(grid) => grid,
                Err(resp) => return resp,
            };
            let cells: std::collections::HashMap<usize, &Vec<String>> =
                grid.rows.iter().map(|r| (r.source_row, &r.cells)).collect();
            let kind_label =
                |slug: &str| kinds.get(slug).cloned().unwrap_or_else(|| slug.to_string());
            let lines: Vec<report::ReportLine> = stored
                .rows
                .iter()
                .filter(|r| r.outcome != RowOutcome::Skipped)
                .filter_map(|r| {
                    let matched: Vec<&imports::DroppedCell> = r
                        .dropped
                        .iter()
                        .filter(|d| {
                            groups.iter().any(|g| {
                                g.column == d.column && g.kind == d.kind && g.value == d.value
                            })
                        })
                        .collect();
                    if matched.is_empty() {
                        return None;
                    }
                    Some(report::ReportLine {
                        row: r.row,
                        errors: report::dropped_text(&matched, &fields, kind_label),
                        cells: cells.get(&r.row).map_or(&[][..], |c| c.as_slice()),
                    })
                })
                .collect();
            report::report_csv(&source.headers, delimiter, lines.into_iter())
        }
    };
    let suffix = match wanted {
        ReportRows::Skipped => "errors",
        _ => "rows",
    };
    match body {
        Ok(body) => HttpResponse::Ok()
            .insert_header((header::CONTENT_TYPE, "text/csv; charset=utf-8"))
            .insert_header((
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}-{suffix}.csv\"",
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
pub(crate) fn download_stem(filename: &str) -> String {
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

// ---- import the next sheet ----

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextSheetRequest {
    pub sheet: String,
}

fn next_sheet_refused(refusal: next_sheet::Refusal) -> HttpResponse {
    use next_sheet::Refusal;
    match refusal {
        Refusal::NotImported => errors::conflict_with_code(
            "This import wasn't committed. Import this sheet before the next one.",
            "import_not_committed",
        ),
        Refusal::Pruned => details_pruned(),
        Refusal::SingleSheet => errors::bad_request_with_code(
            "This file has one sheet, and it was imported.",
            "import_single_sheet",
        ),
        Refusal::UnknownSheet => errors::bad_request_with_code(
            "This workbook has no sheet by that name.",
            "import_sheet_unknown",
        ),
        Refusal::HiddenSheet => errors::bad_request_with_code(
            "That sheet is hidden in the workbook. Unhide it and upload the file again.",
            "import_sheet_hidden",
        ),
        Refusal::AlreadyImported => errors::conflict_with_code(
            "That sheet was already imported from this file.",
            "import_sheet_already_imported",
        ),
    }
}

/// What checking a next sheet against the source job came to.
enum NextSheetCheck {
    Create {
        job: ImportJob,
        job_type: ImportType,
        root: Uuid,
    },
    Existing(ImportJob),
    NotFound,
    Refused(next_sheet::Refusal),
}

/// Import the next sheet: a new draft of the same type on another sheet of
/// a committed import's workbook, with its own copy of the upload, read as
/// `PUT source` reads a chosen sheet (header row guessed). Nothing else is
/// carried; Map suggests the columns from memory. A draft on that sheet
/// from the same upload that can still go on is answered instead of a new
/// one.
pub async fn next_sheet(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    path: web::Path<Uuid>,
    body: web::Json<NextSheetRequest>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let sheet = body.into_inner().sheet;

    let check = tc.run(|conn| {
        let job = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(NextSheetCheck::NotFound),
            Err(e) => return Err(e),
        };
        let root = next_sheet::root_of(job.id, job.upload_of);
        let jobs = repo::jobs_of_upload(conn, root)?;
        let source = stored_source(&job).map(|s| s.source);
        let plan = next_sheet::plan(
            &job.status,
            job.pruned_at.is_some(),
            source.as_ref(),
            &sheet,
            &jobs,
        );
        match plan {
            Err(refusal) => Ok(NextSheetCheck::Refused(refusal)),
            Ok(next_sheet::Plan::Existing(draft)) => {
                Ok(NextSheetCheck::Existing(repo::get(conn, draft)?))
            }
            Ok(next_sheet::Plan::Create) => match ImportType::from_str(&job.job_type) {
                Some(job_type) => Ok(NextSheetCheck::Create {
                    job,
                    job_type,
                    root,
                }),
                None => Ok(NextSheetCheck::Refused(next_sheet::Refusal::NotImported)),
            },
        }
    });
    let (source_job, job_type, root) = match check {
        Ok(NextSheetCheck::Create {
            job,
            job_type,
            root,
        }) => (job, job_type, root),
        Ok(NextSheetCheck::Existing(draft)) => {
            return HttpResponse::Ok().json(ImportJobResponse::from(draft))
        }
        Ok(NextSheetCheck::NotFound) => {
            return errors::not_found_msg(format!("import job {id} not found"))
        }
        Ok(NextSheetCheck::Refused(refusal)) => return next_sheet_refused(refusal),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to load import job");
            return errors::internal("failed to load import job");
        }
    };

    // The new job's own copy of the upload: the source's files go with its
    // prune, and this draft has a window of its own.
    let extension = FileFormat::Xlsx.as_str();
    let bytes = match files::read_upload(storage.0.as_ref(), id, extension).await {
        Ok(b) => b,
        Err(e) => return file_error(id, e),
    };
    let new_id = Uuid::now_v7();
    let stored_key = match files::put_upload(storage.0.as_ref(), new_id, extension, &bytes).await {
        Ok(key) => key,
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to copy an import upload");
            return errors::internal("failed to read import file");
        }
    };
    let choice = SheetChoice {
        sheet: sheet.clone(),
        header_row: None,
    };
    let read = parse_off_worker(move || -> std::io::Result<Result<_, Refused>> {
        match xlsx::read(&bytes, Some(&choice)) {
            Ok(workbook) => Ok(Ok((files::grid_bytes(&workbook.grid)?, workbook.source))),
            Err(refused) => Ok(Err(refused)),
        }
    })
    .await;
    let (grid_bytes, source) = match read {
        Ok(Ok(Ok(read))) => read,
        Ok(Ok(Err(refused))) => {
            remove_files(&storage, new_id).await;
            return refused_response(refused);
        }
        Ok(Err(e)) => {
            error!(job_id = %id, error = ?e, "failed to encode import grid");
            remove_files(&storage, new_id).await;
            return errors::internal("failed to read import file");
        }
        Err(ParseFailure::TimedOut) => {
            remove_files(&storage, new_id).await;
            return refused_response(timed_out());
        }
        Err(ParseFailure::Task(e)) => {
            error!(job_id = %id, error = %e, "import parse task failed");
            remove_files(&storage, new_id).await;
            return errors::internal("failed to read import file");
        }
    };
    let grid = match files::put_grid(storage.0.as_ref(), new_id, &grid_bytes).await {
        Ok(version) => version,
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to store import grid");
            remove_files(&storage, new_id).await;
            return errors::internal("failed to read import file");
        }
    };
    drop(grid_bytes);

    enum Made {
        Created(ImportJob),
        Existing(ImportJob),
        Refused(next_sheet::Refusal),
        Busy,
    }
    let result = tc.run(|conn| {
        // Under the first job's lock, so two presses make one draft.
        if !repo::try_lock_job(conn, root)? {
            return Ok(Made::Busy);
        }
        let jobs = repo::jobs_of_upload(conn, root)?;
        let current = repo::get(conn, source_job.id)?;
        let plan = next_sheet::plan(
            &current.status,
            current.pruned_at.is_some(),
            stored_source(&current).map(|s| s.source).as_ref(),
            &sheet,
            &jobs,
        );
        match plan {
            Err(refusal) => return Ok(Made::Refused(refusal)),
            Ok(next_sheet::Plan::Existing(draft)) => {
                return Ok(Made::Existing(repo::get(conn, draft)?))
            }
            Ok(next_sheet::Plan::Create) => {}
        }
        let job = repo::create(
            conn,
            NewImportJob {
                id: new_id,
                job_type: job_type.as_str().to_string(),
                filename: source_job.filename.clone(),
                file_path: stored_key.clone(),
                created_by: Some(auth.user_uuid),
                upload_of: Some(root),
            },
        )?;
        let stored = suggest_for(conn, job_type, source, Some(grid))?;
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
            "import next sheet started"
        );
        Ok(Made::Created(updated))
    });

    if !matches!(result, Ok(Made::Created(_))) {
        // No job names these files.
        remove_files(&storage, new_id).await;
    }
    match result {
        Ok(Made::Created(job)) => HttpResponse::Created().json(ImportJobResponse::from(job)),
        Ok(Made::Existing(job)) => HttpResponse::Ok().json(ImportJobResponse::from(job)),
        Ok(Made::Refused(refusal)) => next_sheet_refused(refusal),
        Ok(Made::Busy) => errors::conflict_with_code(
            "This import is being changed. Try again in a moment.",
            "import_busy",
        ),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to start the next sheet");
            errors::internal("failed to start the import")
        }
    }
}

/// The sheet Done offers to import next from a committed workbook import
/// whose file is kept.
fn next_sheet_of(
    conn: &mut crate::db::DbConnection,
    job: &ImportJob,
) -> Result<Option<String>, diesel::result::Error> {
    if job.status != "done" || job.pruned_at.is_some() {
        return Ok(None);
    }
    let Some(stored) = stored_source(job) else {
        return Ok(None);
    };
    if stored.source.format != FileFormat::Xlsx.as_str() || stored.source.sheets.len() < 2 {
        return Ok(None);
    }
    let jobs = repo::jobs_of_upload(conn, next_sheet::root_of(job.id, job.upload_of))?;
    Ok(next_sheet::offer(&stored.source, &jobs))
}

// ---- send invites ----

/// What checking Send invites against the job came to.
enum InviteCheck {
    Ok {
        job: ImportJob,
        people: Vec<sign_in::Waiting>,
        names: std::collections::HashMap<Uuid, String>,
        admin_name: String,
    },
    NotFound,
    Refused(HttpResponse),
}

/// Send invites: an invitation email, the one Resend invitation on a
/// user's page sends, to each person a committed users import created who
/// can't sign in yet and holds no open invitation. Only where an invitation
/// sets a password (self-hosted) and email is set up. Answers how many were
/// sent and Done's sign-in line as it stands.
pub async fn send_invites(
    mut tc: TenantConn,
    auth: AuthContext,
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let check = tc.run(|conn| {
        let job = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(InviteCheck::NotFound),
            Err(e) => return Err(e),
        };
        if job.job_type != ImportType::Users.as_str() {
            return Ok(InviteCheck::Refused(errors::bad_request_with_code(
                "Only a users import sends invites.",
                "import_invites_users_only",
            )));
        }
        if job.status != "done" {
            return Ok(InviteCheck::Refused(errors::conflict_with_code(
                "This import wasn't committed, so it created nobody to invite.",
                "import_not_committed",
            )));
        }
        if !crate::middleware::workspace_context::local_credentials_permitted() {
            return Ok(InviteCheck::Refused(errors::conflict_with_code(
                "People here sign in with the workspace's sign-in provider, so there are no invites to send.",
                "import_invites_not_offered",
            )));
        }
        if !sign_in::email_ready() {
            return Ok(InviteCheck::Refused(errors::conflict_with_code(
                "Email isn't set up, so invites can't be sent.",
                "import_invites_email_off",
            )));
        }
        let people: Vec<sign_in::Waiting> =
            sign_in::waiting(conn, &sign_in::created_people(&job))?
                .into_iter()
                .filter(|w| !w.invited)
                .collect();
        if people.is_empty() {
            return Ok(InviteCheck::Refused(errors::conflict_with_code(
                "Everyone this import created can sign in or has an open invitation.",
                "import_nothing_to_invite",
            )));
        }
        let mut uuids: Vec<Uuid> = people.iter().map(|p| p.user).collect();
        uuids.push(auth.user_uuid);
        let names = names_of(conn, uuids)?;
        let admin_name = names
            .get(&auth.user_uuid)
            .cloned()
            .unwrap_or_else(|| "An administrator".to_string());
        Ok(InviteCheck::Ok {
            job,
            people,
            names,
            admin_name,
        })
    });
    let (job, people, names, admin_name) = match check {
        Ok(InviteCheck::Ok {
            job,
            people,
            names,
            admin_name,
        }) => (job, people, names, admin_name),
        Ok(InviteCheck::NotFound) => {
            return errors::not_found_msg(format!("import job {id} not found"))
        }
        Ok(InviteCheck::Refused(response)) => return response,
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to load import job");
            return errors::internal("failed to load import job");
        }
    };

    // The invitation helper takes a connection pinned to the request's
    // workspace, as Resend invitation does.
    let mut conn = match crate::handlers::helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to get a connection for invites");
            return errors::internal("failed to send the invites");
        }
    };
    crate::handlers::helpers::pin_request_workspace(&req, &mut conn);
    let mut sent = 0usize;
    let mut failed = 0usize;
    for person in &people {
        let name = names.get(&person.user).map_or("", String::as_str);
        match crate::handlers::users::send_user_invitation(
            &mut conn,
            &req,
            person.user,
            &person.email,
            name,
            &admin_name,
        )
        .await
        {
            crate::handlers::users::SendInvitationResult::Success
            | crate::handlers::users::SendInvitationResult::Capped => sent += 1,
            other => {
                error!(job_id = %id, error_kind = %other, "failed to send an import invite");
                failed += 1;
            }
        }
    }
    info!(job_id = %id, count = sent, failed, "import invites sent");

    let status = match tc.run(|conn| sign_in::status(conn, &job)) {
        Ok(status) => status,
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to read the sign-in line after invites");
            None
        }
    };
    HttpResponse::Ok().json(serde_json::json!({
        "sent": sent,
        "failed": failed,
        "sign_in": status,
    }))
}

// ---- remove imported assets, previous values ----

/// Why Remove imported assets can't run on a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemovalRefusal {
    /// Users and tickets imports don't offer it.
    AssetsOnly,
    /// A draft, or a commit that failed: nothing was imported.
    NotImported,
    /// It ran already; it runs once.
    AlreadyRemoved,
    /// 30 days after the commit, or the prune dropped the row results the
    /// rule reads.
    WindowClosed,
}

impl RemovalRefusal {
    fn response(self) -> HttpResponse {
        match self {
            Self::AssetsOnly => errors::bad_request_with_code(
                "Only an asset import can remove what it created.",
                "import_removal_assets_only",
            ),
            Self::NotImported => errors::conflict_with_code(
                "This import wasn't committed, so it created nothing to remove.",
                "import_not_committed",
            ),
            Self::AlreadyRemoved => errors::conflict_with_code(
                "The assets this import created were already removed.",
                "import_already_removed",
            ),
            Self::WindowClosed => errors::gone_with_code(
                "Imported assets can be removed for 30 days after the import. That window has closed.",
                "import_removal_window_closed",
            ),
        }
    }
}

/// Whether removal can run on `job` at `now`, before the row results are
/// read.
fn removal_precondition(
    job: &ImportJob,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<RemovalRefusal> {
    if job.job_type != ImportType::Assets.as_str() {
        return Some(RemovalRefusal::AssetsOnly);
    }
    if job.status != "done" {
        return Some(RemovalRefusal::NotImported);
    }
    if job.removed_at.is_some() {
        return Some(RemovalRefusal::AlreadyRemoved);
    }
    let open = job
        .completed_at
        .is_some_and(|at| now < remove::window_closes_at(at));
    if !open || job.pruned_at.is_some() {
        return Some(RemovalRefusal::WindowClosed);
    }
    None
}

/// The committed row results removal reads, or the refusal when they are
/// gone.
fn committed_rows(
    conn: &mut crate::db::DbConnection,
    id: Uuid,
) -> Result<Result<Vec<RowResult>, RemovalRefusal>, diesel::result::Error> {
    let stored: Option<StoredRowResults> =
        repo::row_results(conn, id)?.and_then(|v| serde_json::from_value(v).ok());
    Ok(match stored {
        Some(s) if s.phase == RowPhase::Committed => Ok(s.rows),
        _ => Err(RemovalRefusal::WindowClosed),
    })
}

enum RemovalRead<T> {
    Ok(T),
    NotFound,
    Refused(RemovalRefusal),
}

/// What Remove imported assets would do now, for its dialog: how many
/// assets the import created, how many would be deleted and which are kept
/// and why, and how many of the catalog entries it created could go.
pub async fn removal_preview(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let read = tc.run(|conn| {
        let job = match repo::get(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(RemovalRead::NotFound),
            Err(e) => return Err(e),
        };
        if let Some(refusal) = removal_precondition(&job, chrono::Utc::now()) {
            return Ok(RemovalRead::Refused(refusal));
        }
        let rows = match committed_rows(conn, id)? {
            Ok(rows) => rows,
            Err(refusal) => return Ok(RemovalRead::Refused(refusal)),
        };
        let mut plan = remove::plan(conn, &rows, false)?;
        let editors: Vec<Uuid> = plan
            .kept
            .iter()
            .flat_map(|k| &k.reasons)
            .filter_map(|r| match r {
                remove::KeptReason::Edited { by, .. } => *by,
                _ => None,
            })
            .collect();
        let names = names_of(conn, editors)?;
        for kept in &mut plan.kept {
            for reason in &mut kept.reasons {
                if let remove::KeptReason::Edited { by, by_name, .. } = reason {
                    *by_name = by.and_then(|u| names.get(&u).cloned());
                }
            }
        }
        let closes_at = job.completed_at.map(remove::window_closes_at);
        Ok(RemovalRead::Ok((plan, closes_at)))
    });
    match read {
        Ok(RemovalRead::Ok((plan, closes_at))) => {
            let mut body = serde_json::to_value(&plan).unwrap_or_default();
            body["closes_at"] = serde_json::to_value(closes_at).unwrap_or_default();
            HttpResponse::Ok().json(body)
        }
        Ok(RemovalRead::NotFound) => errors::not_found_msg(format!("import job {id} not found")),
        Ok(RemovalRead::Refused(refusal)) => refusal.response(),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to plan an import removal");
            errors::internal("failed to check the imported assets")
        }
    }
}

enum RemoveOutcome {
    Removed(ImportJob),
    NotFound,
    Refused(RemovalRefusal),
    Busy,
    TypeBusy,
    NothingToRemove(NothingToRemove),
}

/// Why a removal found nothing to delete. The client words it from
/// `args.reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NothingToRemove {
    /// The import only updated assets.
    NoneCreated,
    /// Every asset it created was deleted some other way.
    AllGone,
    /// Every asset it created that still exists has been changed since.
    AllChanged,
}

impl NothingToRemove {
    fn of(plan: &remove::RemovalPlan) -> Self {
        if plan.created == 0 {
            Self::NoneCreated
        } else if plan.gone == plan.created {
            Self::AllGone
        } else {
            Self::AllChanged
        }
    }

    fn response(self) -> HttpResponse {
        let (reason, message) = match self {
            Self::NoneCreated => (
                "none_created",
                "This import created no assets, so there is nothing to remove.",
            ),
            Self::AllGone => (
                "all_gone",
                "The assets this import created were already deleted.",
            ),
            Self::AllChanged => (
                "all_changed",
                "Every asset this import created has been changed since, so nothing was deleted.",
            ),
        };
        errors::with_fields(
            actix_web::http::StatusCode::CONFLICT,
            "import_nothing_to_remove",
            message,
            serde_json::json!({ "args": { "reason": reason } }),
        )
    }
}

/// Remove imported assets: delete the assets the import created that
/// nobody has touched since, each through `delete_device`, and the
/// catalog entries it created that nothing uses any more, then record on
/// the job when, by whom and what happened. Runs once, within 30 days of
/// the commit, under the job's lock and the import type's lock so it
/// never meets a commit.
pub async fn remove_created(
    mut tc: TenantConn,
    auth: AuthContext,
    search_service: web::Data<Arc<SearchService>>,
    path: web::Path<Uuid>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let deleted = remove::DeletedAssets::default();
    let result = tc.run(|conn| {
        if !repo::try_lock_job(conn, id)? {
            return Ok(RemoveOutcome::Busy);
        }
        let job = match repo::get_for_update(conn, id) {
            Ok(j) => j,
            Err(diesel::result::Error::NotFound) => return Ok(RemoveOutcome::NotFound),
            Err(e) => return Err(e),
        };
        if let Some(refusal) = removal_precondition(&job, chrono::Utc::now()) {
            return Ok(RemoveOutcome::Refused(refusal));
        }
        if !repo::try_lock_type(conn, job.workspace_id, &job.job_type)? {
            return Ok(RemoveOutcome::TypeBusy);
        }
        let rows = match committed_rows(conn, id)? {
            Ok(rows) => rows,
            Err(refusal) => return Ok(RemoveOutcome::Refused(refusal)),
        };
        let plan = remove::plan(conn, &rows, true)?;
        if plan.deletable == 0 {
            return Ok(RemoveOutcome::NothingToRemove(NothingToRemove::of(&plan)));
        }
        let outcome = remove::remove(conn, &rows, plan, &deleted)?;
        let value = serde_json::to_value(&outcome)
            .map_err(|e| diesel::result::Error::SerializationError(Box::new(e)))?;
        let updated = repo::record_removal(conn, id, auth.user_uuid, value)?;
        info!(
            job_id = %id,
            assets_deleted = outcome.deleted,
            assets_kept = outcome.kept.len(),
            "removed imported assets"
        );
        Ok(RemoveOutcome::Removed(updated))
    });
    match result {
        Ok(RemoveOutcome::Removed(job)) => {
            // The deletes committed: the search index drops them now.
            for asset in deleted.into_ids() {
                search_service.get_ref().device_deleted(asset);
            }
            match tc.run(|conn| job_response(conn, job.clone())) {
                Ok(response) => HttpResponse::Ok().json(response),
                Err(e) => {
                    error!(job_id = %id, error = ?e, "failed to read names for an import removal");
                    HttpResponse::Ok().json(ImportJobResponse::from(job))
                }
            }
        }
        Ok(RemoveOutcome::NotFound) => errors::not_found_msg(format!("import job {id} not found")),
        Ok(RemoveOutcome::Refused(refusal)) => refusal.response(),
        Ok(RemoveOutcome::Busy) => errors::conflict_with_code(
            "This import is being changed. Try again in a moment.",
            "import_busy",
        ),
        Ok(RemoveOutcome::TypeBusy) => errors::conflict_with_code(
            "Another import of this type is being committed. Try again when it finishes.",
            "import_type_busy",
        ),
        Ok(RemoveOutcome::NothingToRemove(why)) => why.response(),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to remove imported assets");
            errors::internal("Removing the imported assets failed and nothing was deleted.")
        }
    }
}

/// The job as Done and the import detail page show it: [`job_response`]
/// with the size of its previous values file, the sheet to import next and,
/// for users, who can't sign in yet.
fn page_response(
    conn: &mut crate::db::DbConnection,
    job: ImportJob,
) -> Result<ImportJobResponse, diesel::result::Error> {
    let previous = previous_values_of(conn, &job)?;
    let next = next_sheet_of(conn, &job)?;
    let sign_in = sign_in::status(conn, &job)?;
    let mut response = job_response(conn, job)?;
    response.previous_values = previous;
    response.next_sheet = next;
    response.sign_in = sign_in;
    Ok(response)
}

/// The job as a page shows it: with the names of who uploaded it, who
/// removed its assets and who edited the ones kept.
pub(crate) fn job_response(
    conn: &mut crate::db::DbConnection,
    job: ImportJob,
) -> Result<ImportJobResponse, diesel::result::Error> {
    let mut result: Option<remove::RemoveResult> = job
        .remove_result
        .clone()
        .and_then(|v| serde_json::from_value(v).ok());
    let mut people: Vec<Uuid> = job.created_by.into_iter().chain(job.removed_by).collect();
    if let Some(r) = &result {
        people.extend(r.editors());
    }
    people.sort_unstable();
    people.dedup();
    let names = names_of(conn, people)?;
    let created_by_name = job.created_by.and_then(|u| names.get(&u).cloned());
    let removed_by_name = job.removed_by.and_then(|u| names.get(&u).cloned());
    let mut response = ImportJobResponse::from(job);
    response.created_by_name = created_by_name;
    response.removed_by_name = removed_by_name;
    if let Some(r) = result.as_mut() {
        r.name_editors(&names);
        response.remove_result = serde_json::to_value(r).ok();
    }
    Ok(response)
}

/// The previous values file: one row per asset the import updated, keyed
/// as the import matched, with each field it wrote as it was before
/// (`services::imports::previous`). Importing it puts them back. Asset
/// imports only, while the details are kept; 410 after the prune.
pub async fn previous_values(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let id = path.into_inner();
    let (job, stored) = match tc.run(|conn| read_row_results(conn, id)) {
        Ok(RowResultsRead::Ok(job, stored)) => (job, stored),
        Ok(RowResultsRead::NotFound) => {
            return errors::not_found_msg(format!("import job {id} not found"))
        }
        Ok(RowResultsRead::NotChecked) => return errors::conflict(NOT_COMMITTED),
        Ok(RowResultsRead::Pruned) => return details_pruned(),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to load import rows");
            return errors::internal("failed to load import rows");
        }
    };
    if job.job_type != ImportType::Assets.as_str() {
        return errors::bad_request_with_code(
            "Previous values are kept for asset imports only.",
            "import_previous_values_assets_only",
        );
    }
    if job.status != "done" || stored.phase != RowPhase::Committed {
        return errors::conflict(NOT_COMMITTED);
    }
    let key = stored_choices(&job)
        .map(|(_, options)| options.key())
        .unwrap_or_default();
    match previous::csv(&stored.rows, key) {
        Ok(body) => HttpResponse::Ok()
            .insert_header((header::CONTENT_TYPE, "text/csv; charset=utf-8"))
            .insert_header((
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}-previous-values.csv\"",
                    download_stem(&job.filename)
                ),
            ))
            .body(body),
        Err(e) => {
            error!(job_id = %id, error = ?e, "failed to write previous values");
            errors::internal("failed to write the previous values")
        }
    }
}

const NOT_COMMITTED: &str = "this import hasn't been committed, so it changed nothing";

/// The size of a committed asset import's previous values file, while its
/// row results are kept.
fn previous_values_of(
    conn: &mut crate::db::DbConnection,
    job: &ImportJob,
) -> Result<Option<previous::PreviousValues>, diesel::result::Error> {
    if job.job_type != ImportType::Assets.as_str()
        || job.status != "done"
        || job.pruned_at.is_some()
    {
        return Ok(None);
    }
    let Some(stored) = repo::row_results(conn, job.id)?
        .and_then(|v| serde_json::from_value::<StoredRowResults>(v).ok())
        .filter(|s| s.phase == RowPhase::Committed)
    else {
        return Ok(None);
    };
    let key = stored_choices(job)
        .map(|(_, options)| options.key())
        .unwrap_or_default();
    Ok(Some(previous::counts(&stored.rows, key)))
}

#[derive(Debug, Deserialize)]
pub struct TemplateQuery {
    /// Assets only: the kind whose attributes follow the core columns.
    pub kind: Option<String>,
}

/// CSV template for the requested import type: a header row the admin
/// fills in. For assets, the core columns, then the attributes of `?kind=`
/// when given, with a BOM and each header guarded against formulas.
pub async fn template(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<String>,
    query: web::Query<TemplateQuery>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Import requires workspace admin");
    }
    let job_type = match ImportType::from_str(&path) {
        Some(t) => t,
        None => return errors::bad_request("unknown import type"),
    };
    let kind = query
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty());
    let (body, name) = match job_type {
        ImportType::Assets => {
            let headers = match tc.run(|conn| imports::assets::template_headers(conn, kind)) {
                Ok(Some(headers)) => headers,
                Ok(None) => {
                    return errors::bad_request(format!(
                        "unknown kind '{}'",
                        kind.unwrap_or_default()
                    ))
                }
                Err(e) => {
                    error!(error = ?e, "failed to read asset kinds for a template");
                    return errors::internal("failed to build the template");
                }
            };
            let body = match imports::assets::template_csv(&headers) {
                Ok(body) => body,
                Err(e) => {
                    error!(error = ?e, "failed to write an import template");
                    return errors::internal("failed to build the template");
                }
            };
            let name = match kind {
                Some(kind) => format!(
                    "{}.csv",
                    download_stem(&format!("nosdesk-assets-{kind}-template.csv"))
                ),
                None => "nosdesk-assets-template.csv".to_string(),
            };
            (body, name)
        }
        other => {
            if kind.is_some() {
                return errors::bad_request("kind applies to asset templates only");
            }
            let body = imports::importer_for(other).template_headers().join(",") + "\n";
            (
                body.into_bytes(),
                format!("nosdesk-{}-template.csv", other.as_str()),
            )
        }
    };
    HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "text/csv; charset=utf-8"))
        .insert_header((
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{name}\""),
        ))
        .body(body)
}

// ---- helpers ----

async fn read_first_file_field(payload: &mut Multipart) -> Result<(String, Vec<u8>), Refused> {
    // The wizard ships exactly one file field; extra parts are
    // ignored. Take the first one and return.
    let mut field = match payload.try_next().await {
        Ok(Some(field)) => field,
        _ => {
            return Err(Refused::new(
                "import_no_file",
                "missing file field in upload",
            ))
        }
    };

    let filename = field
        .content_disposition()
        .get_filename()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "upload.csv".to_string());

    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = field.next().await {
        let chunk = chunk
            .map_err(|e| Refused::new("import_upload_unreadable", format!("read error: {e}")))?;
        bytes.extend_from_slice(&chunk);
        // 10 MB cap, well above typical single-tenant import
        // volumes, well below our memory budget. Larger files
        // belong in the Phase 2 background-worker path.
        if bytes.len() > 10 * 1024 * 1024 {
            return Err(
                Refused::new("import_file_too_large", "file exceeds the 10 MB upload cap")
                    .arg("max", 10),
            );
        }
    }
    Ok((filename, bytes))
}

/// The upload's name as stored: `import_jobs.filename` holds 255
/// characters.
pub(crate) fn stored_filename(name: &str) -> String {
    name.chars().take(255).collect()
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

pub(crate) const EXPIRED: &str =
    "This draft expired 30 days after its last change and can't go on. Upload the file again.";

/// A draft past the window or pruned: it can't be changed or committed.
pub(crate) fn expired(job: &ImportJob) -> bool {
    history::state_of(
        &job.status,
        job.updated_at,
        job.pruned_at,
        chrono::Utc::now(),
    ) == Some(JobState::Expired)
}

const DETAILS_PRUNED: &str = "This import's details were kept for 30 days and have been removed. Its counts, options and mapping are kept.";

/// The answer for a job whose per-row detail the prune dropped.
pub(crate) fn details_pruned() -> HttpResponse {
    errors::gone_with_code(DETAILS_PRUNED, "import_details_pruned")
}
