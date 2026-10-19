//! Documentation import routes, beside the spreadsheet importer's
//! (`handlers::imports`), on the same jobs.
//!
//! - `POST /admin/import?type=documentation` takes a multipart upload: one
//!   `.zip`, or one or more loose `.md` files (gathered into a zip), up to
//!   200 MB, streamed to storage. It answers with the job in `parsed`, its
//!   `source.tree` the pages as Nosdesk will show them under the default
//!   options, or 400 with the refusal's code.
//! - `PUT /admin/import/{id}/docs/options` takes `{options, check}`. With
//!   `check: false` it reads the tree again under the options (a changed
//!   profile, a pattern left out) and answers 200 with the job in `parsed`.
//!   Otherwise it starts the check in the background and answers 202 with
//!   the job in `checking`; the job lands in `dry_run_done` (or back in
//!   `parsed` with `error_message` when the file can't be imported so).
//! - `GET /admin/import/{id}/docs/pages?filter=all|notes|skipped|errors&offset=&limit=`
//!   pages through every page's result (25 by default).
//! - `GET /admin/import/{id}/docs/pages/{index}/preview` returns one page as
//!   ProseMirror JSON with its images as named placeholders.
//! - `GET /admin/import/{id}/docs/report` downloads the conversion report.
//! - `POST /admin/import/{id}/docs/commit` takes `{duplicates?}`, checks the
//!   plan still holds and starts the commit in the background: 202 with the
//!   job in `committing` (`records_committed` counts pages written), 409
//!   `import_plan_changed` with the job holding the new plan.
//!
//! - `GET /admin/import/{id}/docs/remove` says what Remove imported pages
//!   would do now: how many pages the import created, how many would move
//!   to Trash, which are kept and why, and which collections it created
//!   would be deleted. `POST` does it, once, within 30 days of the commit,
//!   and answers with the job, its `remove_result` filled.
//! - `GET /admin/import/{id}/docs/publish` says how many of the import's
//!   pages Publish all would publish and how many are left as they are;
//!   `POST` publishes them and answers with the same two counts.
//!
//! `GET /admin/import/{id}` follows a job through `checking` and
//! `committing`, and puts back one whose process died. Every route is
//! workspace-admin only; another workspace's job is 404.

use std::sync::Arc;

use actix_multipart::Multipart;
use actix_web::{http::header, web, HttpRequest, HttpResponse};
use futures::TryStreamExt;
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

use crate::errors;
use crate::extractors::{AuthContext, ScopedStorage, TenantConn, WorkspaceContext};
use crate::handlers::imports::{
    details_pruned, download_stem, expired, job_response, names_of, refused_response,
    stored_filename, EXPIRED,
};
use crate::models::{ImportJob, ImportJobUpdate, NewImportJob};
use crate::repository::documentation::PageAudience;
use crate::repository::documentation_imports as docs_repo;
use crate::repository::imports as repo;
use crate::services::docs_markdown::links::NoWorkspace;
use crate::services::docs_markdown::tree::CollectionMode;
use crate::services::imports::documentation::{
    self as docs, check, commit, reconcile, remove, report, CommitRecord, DocsOptions, DocsRows,
    DocsSource, Duplicates, PageOutcome, TreeView, JOB_TYPE, MAX_UPLOAD_BYTES,
};
use crate::services::imports::refusal::Refused;
use crate::services::search::{indexing_tasks, SearchService};
use crate::services::uploaded_archive::{spool, ArchiveError, ArchiveOptions, SafeArchive};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/import/{id}/docs/options",
        web::put().to(put_options),
    )
    .route("/admin/import/{id}/docs/pages", web::get().to(pages))
    .route(
        "/admin/import/{id}/docs/pages/{index}/preview",
        web::get().to(preview),
    )
    .route("/admin/import/{id}/docs/report", web::get().to(report_csv))
    .route("/admin/import/{id}/docs/commit", web::post().to(commit_job))
    .service(
        web::resource("/admin/import/{id}/docs/remove")
            .route(web::get().to(removal_preview))
            .route(web::post().to(remove_pages)),
    )
    .service(
        web::resource("/admin/import/{id}/docs/publish")
            .route(web::get().to(publish_preview))
            .route(web::post().to(publish_all)),
    );
}

/// The upload cap, when an app sets one other than [`MAX_UPLOAD_BYTES`]
/// (the tests, to send a file past it).
#[derive(Debug, Clone, Copy)]
pub struct UploadCap(pub u64);

const FORBIDDEN: &str = "Import requires workspace admin";

fn not_found(id: Uuid) -> HttpResponse {
    errors::not_found_msg(format!("import job {id} not found"))
}

fn not_docs() -> HttpResponse {
    errors::bad_request("this isn't a documentation import")
}

/// An upload gathered into one zip on local disk.
struct Upload {
    file: tempfile::NamedTempFile,
    filename: String,
}

fn not_a_zip() -> Refused {
    ArchiveError::NotAZip { part: None }.into()
}

fn is_markdown_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".md", ".markdown", ".mdx"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// A loose file's name inside the zip made for it: its last segment, safe
/// as an archive key, unique among the others.
fn entry_name(raw: &str, taken: &mut std::collections::HashSet<String>) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let mut name: String = last
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .trim_start_matches('.')
        .to_string();
    if name.is_empty() || name.len() > 200 {
        name = "page.md".to_string();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) => (stem.to_string(), format!(".{ext}")),
        None => (name.clone(), String::new()),
    };
    let mut candidate = name;
    let mut n = 2;
    while !taken.insert(candidate.to_lowercase()) {
        candidate = format!("{stem} ({n}){ext}");
        n += 1;
    }
    candidate
}

/// Read the upload to local disk under the cap: a zip as it is, or loose
/// Markdown files gathered into one.
async fn read_upload(payload: &mut Multipart, cap: u64) -> Result<Upload, Refused> {
    let mut first = match payload.try_next().await {
        Ok(Some(field)) => field,
        _ => {
            return Err(Refused::new(
                "import_no_file",
                "missing file field in upload",
            ))
        }
    };
    let spooled = spool::spool_field(&mut first, cap, None).await?;
    // The next field waits until this one is dropped.
    drop(first);
    let filename = spooled
        .filename
        .clone()
        .unwrap_or_else(|| "upload.zip".to_string());
    let mut magic = [0u8; 4];
    let is_zip = {
        use std::io::Read;
        let mut f = spooled
            .open()
            .map_err(|e| Refused::new("upload_not_saved", e.to_string()))?;
        f.read(&mut magic).unwrap_or(0) == 4 && magic == *b"PK\x03\x04"
    };
    if is_zip {
        return Ok(Upload {
            file: spooled.into_temp(),
            filename,
        });
    }
    if !is_markdown_name(&filename) {
        return Err(not_a_zip());
    }

    // Loose Markdown: every file field, within the one cap.
    let mut files = vec![(filename.clone(), spooled)];
    let mut total = files[0].1.size;
    while let Some(mut field) = payload
        .try_next()
        .await
        .map_err(|e| Refused::new("upload_unreadable", e.to_string()))?
    {
        let remaining = cap.saturating_sub(total);
        let more = spool::spool_field(&mut field, remaining, None)
            .await
            .map_err(|e| match e {
                spool::SpoolError::TooLarge { .. } => spool::SpoolError::TooLarge { max: cap },
                e => e,
            })?;
        drop(field);
        let name = more.filename.clone().unwrap_or_default();
        if !is_markdown_name(&name) {
            return Err(Refused::new(
                "docs_import_mixed_files",
                "Choose one .zip, or only .md files.",
            ));
        }
        total += more.size;
        files.push((name, more));
    }
    let out = tempfile::NamedTempFile::new()
        .map_err(|e| Refused::new("upload_not_saved", e.to_string()))?;
    let writer = out
        .reopen()
        .map_err(|e| Refused::new("upload_not_saved", e.to_string()))?;
    let written = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(writer);
        let mut taken = std::collections::HashSet::new();
        for (name, spooled) in files {
            zip.start_file(
                entry_name(&name, &mut taken),
                zip::write::SimpleFileOptions::default(),
            )
            .map_err(std::io::Error::other)?;
            let mut source = spooled.open()?;
            std::io::copy(&mut source, &mut zip)?;
        }
        zip.finish().map_err(std::io::Error::other)?.flush()
    })
    .await
    .map_err(|e| Refused::new("upload_not_saved", e.to_string()))?;
    written.map_err(|e| Refused::new("upload_not_saved", e.to_string()))?;
    let filename = if filename.contains('.') {
        filename
    } else {
        "pages.md".to_string()
    };
    Ok(Upload {
        file: out,
        filename,
    })
}

/// Open a zip on disk and read its tree under `options`, off the worker,
/// within the conversion budget.
async fn read_tree(
    file: std::fs::File,
    options: DocsOptions,
    archive_name: String,
) -> Result<(TreeView, usize), Refused> {
    let task = tokio::task::spawn_blocking(move || -> Result<(TreeView, usize), Refused> {
        let mut archive = SafeArchive::open(file, &ArchiveOptions::documentation())?;
        let files = archive.entries().len();
        let built = crate::services::docs_markdown::tree::build(
            &mut archive,
            &options.tree_options(&archive_name),
            &NoWorkspace,
        )?;
        Ok((TreeView::of(&built.plan, &built.documents), files))
    });
    match tokio::time::timeout(docs::CONVERT_BUDGET, task).await {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => Err(Refused::new("docs_import_unreadable", e.to_string())),
        Err(_) => Err(check::timed_out()),
    }
}

/// `POST /admin/import?type=documentation`, from `handlers::imports::upload`
/// once it has checked the caller is an admin.
pub async fn upload(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    req: HttpRequest,
    mut payload: Multipart,
) -> HttpResponse {
    let cap = req
        .app_data::<web::Data<UploadCap>>()
        .map_or(MAX_UPLOAD_BYTES, |c| c.0);
    let upload = match read_upload(&mut payload, cap).await {
        Ok(u) => u,
        Err(refused) => return refused_response(refused),
    };
    let job_id = Uuid::now_v7();
    let size = upload
        .file
        .as_file()
        .metadata()
        .map(|m| m.len())
        .unwrap_or(0);
    let key = docs::upload_key(job_id);
    if let Err(e) = storage
        .0
        .put_file_from_path(upload.file.path(), &key, "application/zip")
        .await
    {
        error!(error = ?e, "failed to store a documentation upload");
        return errors::internal("failed to save uploaded file");
    }
    let file = match upload.file.reopen() {
        Ok(f) => f,
        Err(e) => {
            error!(error = ?e, "failed to reopen a documentation upload");
            remove_files(&storage, job_id).await;
            return errors::internal("failed to read uploaded file");
        }
    };
    let (tree, files) = match read_tree(file, DocsOptions::default(), upload.filename.clone()).await
    {
        Ok(v) => v,
        Err(refused) => {
            // No job names the upload.
            remove_files(&storage, job_id).await;
            return refused_response(refused);
        }
    };
    let source = DocsSource {
        kind: JOB_TYPE.to_string(),
        size,
        files,
        tree,
    };
    let user = auth.user_uuid;
    let filename = stored_filename(&upload.filename);
    let created = tc.run(|conn| {
        let job = repo::create(
            conn,
            NewImportJob {
                id: job_id,
                job_type: JOB_TYPE.to_string(),
                filename,
                file_path: key,
                created_by: Some(user),
                upload_of: None,
            },
        )?;
        let job = repo::update(
            conn,
            job.id,
            ImportJobUpdate {
                status: Some("parsed".to_string()),
                source: Some(serde_json::to_value(&source).ok()),
                options: Some(serde_json::to_value(DocsOptions::default()).ok()),
                ..Default::default()
            },
        )?;
        job_response(conn, job)
    });
    match created {
        Ok(job) => {
            info!(id = %job_id, count = source.tree.pages.len(), "documentation import read");
            HttpResponse::Ok().json(job)
        }
        Err(e) => {
            error!(error = ?e, "failed to start a documentation import");
            remove_files(&storage, job_id).await;
            errors::internal("failed to start import")
        }
    }
}

async fn remove_files(storage: &ScopedStorage, id: Uuid) {
    if let Err(e) = crate::services::imports::files::remove_job_files(storage.0.as_ref(), id).await
    {
        error!(id = %id, error = ?e, "failed to remove import files");
    }
}

/// Load a documentation job, putting it back first when its background run
/// died. `Err` is the response that refuses it.
async fn load(
    tc: &mut TenantConn,
    storage: &ScopedStorage,
    id: Uuid,
) -> Result<ImportJob, HttpResponse> {
    let read = tc.run(|conn| {
        let job = repo::get(conn, id)?;
        let keys = if reconcile::stalled(&job, chrono::Utc::now()) {
            reconcile::settle(conn, id)?
        } else {
            Vec::new()
        };
        Ok((repo::get(conn, id)?, keys))
    });
    match read {
        Ok((job, keys)) => {
            reconcile::remove_stored(storage.get(), keys).await;
            if job.job_type != JOB_TYPE {
                return Err(not_docs());
            }
            Ok(job)
        }
        Err(diesel::result::Error::NotFound) => Err(not_found(id)),
        Err(e) => {
            error!(id = %id, error = ?e, "failed to load import job");
            Err(errors::internal("failed to load import job"))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionsRequest {
    #[serde(default)]
    pub options: DocsOptions,
    /// Run the check (the default), or only read the tree again.
    #[serde(default = "yes")]
    pub check: bool,
}

fn yes() -> bool {
    true
}

/// Why the options' destination can't be used, if it can't.
fn destination_problem(
    conn: &mut crate::db::DbConnection,
    audience: &PageAudience,
    options: &DocsOptions,
) -> diesel::QueryResult<Option<&'static str>> {
    if options.collections != Some(CollectionMode::Existing) {
        return Ok(None);
    }
    let Some(collection) = options.collection_id else {
        return Ok(Some("Choose the collection to import into."));
    };
    if !docs_repo::collection_exists(conn, collection)?
        || !audience.can_open_collection(conn, collection)?
    {
        return Ok(Some("That collection doesn't exist."));
    }
    if let Some(page) = options.under_page_id {
        let in_it = docs_repo::pages_by_id(conn, &[page])?
            .first()
            .is_some_and(|p| p.collection_id == Some(collection));
        if !in_it {
            return Ok(Some("That page isn't in the collection."));
        }
    }
    Ok(None)
}

/// `PUT /admin/import/{id}/docs/options`
pub async fn put_options(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    workspace: WorkspaceContext,
    path: web::Path<Uuid>,
    body: web::Json<OptionsRequest>,
) -> HttpResponse {
    if !auth.is_workspace_admin() {
        return errors::forbidden(FORBIDDEN);
    }
    let id = path.into_inner();
    let OptionsRequest { options, check } = body.into_inner();
    let job = match load(&mut tc, &storage, id).await {
        Ok(job) => job,
        Err(resp) => return resp,
    };
    if let Some(message) = change_refused(&job) {
        return errors::conflict_with_code(message.0, message.1);
    }
    if let Some(message) = options.invalid() {
        return errors::bad_request(message);
    }
    let audience = PageAudience::from_auth(&auth);
    match tc.run(|conn| destination_problem(conn, &audience, &options)) {
        Ok(None) => {}
        Ok(Some(message)) => return errors::bad_request(message),
        Err(e) => {
            error!(id = %id, error = ?e, "failed to check an import's destination");
            return errors::internal("failed to check the import");
        }
    }

    if !check {
        return reread_tree(tc, storage, job, options).await;
    }

    // The check runs in the background; the job says so meanwhile.
    let run = Uuid::now_v7();
    let stored_options = serde_json::to_value(&options).ok();
    let started = tc.run(|conn| {
        if !repo::try_lock_job(conn, id)? {
            return Ok(None);
        }
        let job = repo::get_for_update(conn, id)?;
        if change_refused(&job).is_some() {
            return Ok(None);
        }
        let kept = DocsRows::from_value(repo::row_results(conn, id)?)
            .map(|r| r.web_images)
            .unwrap_or_default();
        let updated = repo::update(
            conn,
            id,
            ImportJobUpdate {
                status: Some("checking".to_string()),
                options: Some(stored_options),
                summary: Some(Some(check::running_summary(run))),
                row_results: Some(None),
                error_message: Some(None),
                ..Default::default()
            },
        )?;
        Ok(Some((updated, kept)))
    });
    let (job, kept_web) = match started {
        Ok(Some(v)) => v,
        Ok(None) => {
            return errors::conflict_with_code(
                "This import is being checked or committed.",
                "import_job_busy",
            )
        }
        Err(e) => {
            error!(id = %id, error = ?e, "failed to start a documentation check");
            return errors::internal("failed to check the import");
        }
    };
    check::spawn(check::CheckRun {
        tc: tc.clone(),
        storage: storage.get(),
        job_id: id,
        run,
        workspace_uuid: workspace.workspace_uuid,
        filename: job.filename.clone(),
        options,
        kept_web,
    });
    info!(id = %id, "documentation check started");
    respond(&mut tc, job, actix_web::http::StatusCode::ACCEPTED)
}

/// Why the job's options can't change now: (message, code).
fn change_refused(job: &ImportJob) -> Option<(&'static str, &'static str)> {
    if job.pruned_at.is_some() || expired(job) {
        return Some((EXPIRED, "import_expired"));
    }
    match job.status.as_str() {
        "parsed" | "dry_run_done" => None,
        "checking" => Some(("This import is being checked.", "import_check_running")),
        "committing" => Some(("This import is being committed.", "import_commit_running")),
        _ => Some((
            "This import has been committed; its options can't change.",
            "import_committed",
        )),
    }
}

fn respond(
    tc: &mut TenantConn,
    job: ImportJob,
    status: actix_web::http::StatusCode,
) -> HttpResponse {
    match tc.run(|conn| job_response(conn, job.clone())) {
        Ok(response) => HttpResponse::build(status).json(response),
        Err(e) => {
            error!(error = ?e, "failed to read an import's page");
            HttpResponse::build(status).json(crate::models::ImportJobResponse::from(job))
        }
    }
}

/// Read the tree again under new options: back to `parsed`, no check.
async fn reread_tree(
    mut tc: TenantConn,
    storage: ScopedStorage,
    job: ImportJob,
    options: DocsOptions,
) -> HttpResponse {
    let id = job.id;
    let spooled = match spool::fetch(storage.0.as_ref(), &docs::upload_key(id), None).await {
        Ok(s) => s,
        Err(crate::utils::storage::StorageError::NotFound(_)) => {
            return errors::conflict_with_code(
                crate::services::imports::files::MISSING,
                crate::services::imports::files::MISSING_CODE,
            )
        }
        Err(e) => {
            error!(id = %id, error = ?e, "failed to read a documentation upload");
            return errors::internal("failed to read the import's file");
        }
    };
    let file = match spooled.open() {
        Ok(f) => f,
        Err(e) => {
            error!(id = %id, error = ?e, "failed to open a documentation upload");
            return errors::internal("failed to read the import's file");
        }
    };
    let (tree, files) = match read_tree(file, options.clone(), job.filename.clone()).await {
        Ok(v) => v,
        Err(refused) => return refused_response(refused),
    };
    let mut source = DocsSource::from_job(&job).unwrap_or(DocsSource {
        kind: JOB_TYPE.to_string(),
        size: spooled.size,
        files,
        tree: tree.clone(),
    });
    source.tree = tree;
    let updated = tc.run(|conn| {
        if !repo::try_lock_job(conn, id)? {
            return Ok(None);
        }
        let current = repo::get_for_update(conn, id)?;
        if change_refused(&current).is_some() {
            return Ok(None);
        }
        repo::update(
            conn,
            id,
            ImportJobUpdate {
                status: Some("parsed".to_string()),
                options: Some(serde_json::to_value(&options).ok()),
                source: Some(serde_json::to_value(&source).ok()),
                summary: Some(None),
                row_results: Some(None),
                error_message: Some(None),
                ..Default::default()
            },
        )
        .map(Some)
    });
    match updated {
        Ok(Some(job)) => respond(&mut tc, job, actix_web::http::StatusCode::OK),
        Ok(None) => errors::conflict_with_code(
            "This import is being checked or committed.",
            "import_job_busy",
        ),
        Err(e) => {
            error!(id = %id, error = ?e, "failed to store a documentation tree");
            errors::internal("failed to read the import's file")
        }
    }
}

const NOT_CHECKED: &str = "this import hasn't been checked yet; check the pages first";

/// A checked job's page results, or the response that refuses them.
fn checked_rows(
    conn_result: diesel::QueryResult<Option<serde_json::Value>>,
    job: &ImportJob,
) -> Result<DocsRows, Box<HttpResponse>> {
    if job.pruned_at.is_some() {
        return Err(Box::new(details_pruned()));
    }
    let value = match conn_result {
        Ok(v) => v,
        Err(e) => {
            error!(id = %job.id, error = ?e, "failed to read an import's pages");
            return Err(Box::new(errors::internal(
                "failed to read the import's pages",
            )));
        }
    };
    match DocsRows::from_value(value) {
        Some(rows) if matches!(job.status.as_str(), "dry_run_done" | "committing" | "done") => {
            Ok(rows)
        }
        _ => Err(Box::new(errors::conflict_with_code(
            NOT_CHECKED,
            "import_not_checked",
        ))),
    }
}

#[derive(Debug, Deserialize)]
pub struct PagesQuery {
    pub filter: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

/// `GET /admin/import/{id}/docs/pages`
pub async fn pages(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    path: web::Path<Uuid>,
    query: web::Query<PagesQuery>,
) -> HttpResponse {
    if !auth.is_workspace_admin() {
        return errors::forbidden(FORBIDDEN);
    }
    let id = path.into_inner();
    let job = match load(&mut tc, &storage, id).await {
        Ok(job) => job,
        Err(resp) => return resp,
    };
    let rows = match checked_rows(tc.run(|conn| repo::row_results(conn, id)), &job) {
        Ok(rows) => rows,
        Err(resp) => return *resp,
    };
    let filter = query.filter.as_deref().unwrap_or("all");
    let keep = |p: &docs::PageRow| match filter {
        "notes" => p.has_notes(),
        "skipped" => p.outcome == PageOutcome::Skipped,
        "errors" => p.outcome == PageOutcome::Error,
        _ => true,
    };
    if !matches!(filter, "all" | "notes" | "skipped" | "errors") {
        return errors::bad_request("filter is all, notes, skipped or errors");
    }
    let matching: Vec<&docs::PageRow> = rows.pages.iter().filter(|p| keep(p)).collect();
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(25).clamp(1, 200);
    let page: Vec<&docs::PageRow> = matching.iter().skip(offset).take(limit).copied().collect();
    HttpResponse::Ok().json(serde_json::json!({
        "total": matching.len(),
        "offset": offset,
        "limit": limit,
        "counts": {
            "all": rows.pages.len(),
            "notes": rows.pages.iter().filter(|p| p.has_notes()).count(),
            "skipped": rows.pages.iter().filter(|p| p.outcome == PageOutcome::Skipped).count(),
            "errors": rows.pages.iter().filter(|p| p.outcome == PageOutcome::Error).count(),
        },
        "pages": page,
    }))
}

/// `GET /admin/import/{id}/docs/pages/{index}/preview`
pub async fn preview(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    path: web::Path<(Uuid, usize)>,
) -> HttpResponse {
    if !auth.is_workspace_admin() {
        return errors::forbidden(FORBIDDEN);
    }
    let (id, index) = path.into_inner();
    let job = match load(&mut tc, &storage, id).await {
        Ok(job) => job,
        Err(resp) => return resp,
    };
    if job.pruned_at.is_some() {
        return details_pruned();
    }
    if !matches!(job.status.as_str(), "dry_run_done" | "committing" | "done") {
        return errors::conflict_with_code(NOT_CHECKED, "import_not_checked");
    }
    match storage.0.get_file(&docs::preview_key(id, index)).await {
        Ok(bytes) => HttpResponse::Ok()
            .insert_header((header::CONTENT_TYPE, "application/json"))
            .body(bytes),
        Err(crate::utils::storage::StorageError::NotFound(_)) => {
            errors::not_found_msg(format!("page {index} isn't in this import"))
        }
        Err(e) => {
            error!(id = %id, error = ?e, "failed to read an import preview");
            errors::internal("failed to read the preview")
        }
    }
}

/// `GET /admin/import/{id}/docs/report`
pub async fn report_csv(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    path: web::Path<Uuid>,
) -> HttpResponse {
    if !auth.is_workspace_admin() {
        return errors::forbidden(FORBIDDEN);
    }
    let id = path.into_inner();
    let job = match load(&mut tc, &storage, id).await {
        Ok(job) => job,
        Err(resp) => return resp,
    };
    let rows = match checked_rows(tc.run(|conn| repo::row_results(conn, id)), &job) {
        Ok(rows) => rows,
        Err(resp) => return *resp,
    };
    match report::report_csv(&rows) {
        Ok(body) => HttpResponse::Ok()
            .insert_header((header::CONTENT_TYPE, "text/csv; charset=utf-8"))
            .insert_header((
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}-conversion-report.csv\"",
                    download_stem(&job.filename)
                ),
            ))
            .body(body),
        Err(e) => {
            error!(id = %id, error = ?e, "failed to write a conversion report");
            errors::internal("failed to write the conversion report")
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitRequest {
    pub duplicates: Option<Duplicates>,
}

/// `POST /admin/import/{id}/docs/commit`
pub async fn commit_job(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    workspace: WorkspaceContext,
    search: web::Data<Arc<SearchService>>,
    path: web::Path<Uuid>,
    body: Option<web::Json<CommitRequest>>,
) -> HttpResponse {
    if !auth.is_workspace_admin() {
        return errors::forbidden(FORBIDDEN);
    }
    let id = path.into_inner();
    let choice = body.and_then(|b| b.into_inner().duplicates);
    let job = match load(&mut tc, &storage, id).await {
        Ok(job) => job,
        Err(resp) => return resp,
    };
    let run = Uuid::now_v7();
    let expired_reason =
        |job: &ImportJob| (job.pruned_at.is_some() || expired(job)).then(|| EXPIRED.to_string());
    let started = tc.run(|conn| commit::start(conn, id, run, choice, expired_reason));
    match started {
        Ok(commit::Start::Started(job)) => {
            commit::spawn(commit::CommitRun {
                tc: tc.clone(),
                storage: storage.get(),
                search: search.get_ref().clone(),
                job_id: id,
                run,
                actor: auth.user_uuid,
                workspace_uuid: workspace.workspace_uuid,
                filename: job.filename.clone(),
            });
            info!(id = %id, "documentation commit started");
            respond(&mut tc, job, actix_web::http::StatusCode::ACCEPTED)
        }
        Ok(commit::Start::Done(job)) => respond(&mut tc, job, actix_web::http::StatusCode::OK),
        Ok(commit::Start::PlanChanged(job, message)) => {
            let response = tc
                .run(|conn| job_response(conn, job.clone()))
                .unwrap_or_else(|_| crate::models::ImportJobResponse::from(job));
            errors::with_fields(
                actix_web::http::StatusCode::CONFLICT,
                "import_plan_changed",
                message,
                serde_json::json!({ "job": response }),
            )
        }
        Ok(commit::Start::Busy(message, code)) => errors::conflict_with_code(message, code),
        Ok(commit::Start::NotReady(message)) => errors::bad_request(message),
        Err(diesel::result::Error::NotFound) => not_found(id),
        Err(e) => {
            error!(id = %job.id, error = ?e, "failed to start a documentation commit");
            errors::internal("failed to start the import")
        }
    }
}

// ---- remove imported pages, publish all ----

/// Why Remove imported pages or Publish all can't run on a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// A draft, or a commit that failed: nothing was imported.
    NotImported,
    /// Removal ran already; it runs once.
    AlreadyRemoved,
    /// 30 days after the commit.
    WindowClosed,
    /// The job's lock is held: a commit or another removal runs.
    Busy,
    /// Another documentation import is being committed.
    TypeBusy,
}

impl Refusal {
    fn response(self) -> HttpResponse {
        match self {
            Self::NotImported => errors::conflict_with_code(
                "This import wasn't committed, so it created no pages.",
                "import_not_committed",
            ),
            Self::AlreadyRemoved => errors::conflict_with_code(
                "The pages this import created were already moved to Trash.",
                "import_already_removed",
            ),
            Self::WindowClosed => errors::gone_with_code(
                "Imported pages can be removed for 30 days after the import. That window has closed.",
                "import_removal_window_closed",
            ),
            Self::Busy => errors::conflict_with_code(
                "This import is being changed. Try again in a moment.",
                "import_busy",
            ),
            Self::TypeBusy => errors::conflict_with_code(
                "Another documentation import is being committed. Try again when it finishes.",
                "import_type_busy",
            ),
        }
    }
}

/// The pages a committed job wrote, or why there are none to act on.
fn committed_record(job: &ImportJob) -> Result<CommitRecord, Refusal> {
    match CommitRecord::from_value(job.record_ids.clone()) {
        Some(record) if job.status == "done" && !record.pending => Ok(record),
        _ => Err(Refusal::NotImported),
    }
}

/// Whether removal can run on `job` at `now`: the pages it wrote, or the
/// refusal.
fn removal_record(
    job: &ImportJob,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<CommitRecord, Refusal> {
    let record = committed_record(job)?;
    if job.removed_at.is_some() {
        return Err(Refusal::AlreadyRemoved);
    }
    let open = job
        .completed_at
        .is_some_and(|at| now < crate::services::imports::remove::window_closes_at(at));
    if !open {
        return Err(Refusal::WindowClosed);
    }
    Ok(record)
}

/// Why a removal found nothing to move to Trash. The client words it from
/// `args.reason`.
fn nothing_to_remove(plan: &remove::RemovalPlan) -> HttpResponse {
    let (reason, message) = if plan.created == 0 {
        (
            "none_created",
            "This import created no pages, so there is nothing to remove.",
        )
    } else if plan.gone == plan.created {
        (
            "all_gone",
            "The pages this import created were already deleted or moved to Trash.",
        )
    } else {
        (
            "all_changed",
            "Every page this import created has been changed since, so nothing was moved to Trash.",
        )
    };
    errors::with_fields(
        actix_web::http::StatusCode::CONFLICT,
        "import_nothing_to_remove",
        message,
        serde_json::json!({ "args": { "reason": reason } }),
    )
}

enum Read<T> {
    Ok(T),
    Refused(Refusal),
}

/// `GET /admin/import/{id}/docs/remove`: what Remove imported pages would
/// do now, for its dialog, with `closes_at`.
pub async fn removal_preview(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    path: web::Path<Uuid>,
) -> HttpResponse {
    if !auth.is_workspace_admin() {
        return errors::forbidden(FORBIDDEN);
    }
    let id = path.into_inner();
    let job = match load(&mut tc, &storage, id).await {
        Ok(job) => job,
        Err(resp) => return resp,
    };
    let record = match removal_record(&job, chrono::Utc::now()) {
        Ok(record) => record,
        Err(refusal) => return refusal.response(),
    };
    let read = tc.run(|conn| {
        let mut plan = remove::plan(conn, &record, false)?;
        let names = names_of(conn, remove::editors_of(&plan.kept))?;
        remove::name_editors(&mut plan.kept, &names);
        Ok::<_, diesel::result::Error>(plan)
    });
    match read {
        Ok(plan) => {
            let mut body = serde_json::to_value(&plan).unwrap_or_default();
            body["closes_at"] = serde_json::to_value(
                job.completed_at
                    .map(crate::services::imports::remove::window_closes_at),
            )
            .unwrap_or_default();
            HttpResponse::Ok().json(body)
        }
        Err(e) => {
            error!(id = %id, error = ?e, "failed to plan removing imported pages");
            errors::internal("failed to check the imported pages")
        }
    }
}

enum RemovalRun {
    Done(ImportJob, Vec<i32>),
    Refused(Refusal),
    Nothing(remove::RemovalPlan),
}

/// `POST /admin/import/{id}/docs/remove`: move the pages the import created
/// that nobody has touched since to Trash, delete the collections it
/// created that end up empty, and record on the job when, by whom and what
/// happened. Runs once, within 30 days of the commit, under the job's lock
/// and the documentation import lock, so it never meets a commit.
pub async fn remove_pages(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    search: web::Data<Arc<SearchService>>,
    path: web::Path<Uuid>,
) -> HttpResponse {
    if !auth.is_workspace_admin() {
        return errors::forbidden(FORBIDDEN);
    }
    let id = path.into_inner();
    if let Err(resp) = load(&mut tc, &storage, id).await {
        return resp;
    }
    let result = tc.run(|conn| {
        if !repo::try_lock_job(conn, id)? {
            return Ok(RemovalRun::Refused(Refusal::Busy));
        }
        let job = repo::get_for_update(conn, id)?;
        let record = match removal_record(&job, chrono::Utc::now()) {
            Ok(record) => record,
            Err(refusal) => return Ok(RemovalRun::Refused(refusal)),
        };
        if !repo::try_lock_type(conn, job.workspace_id, JOB_TYPE)?
            || repo::other_documentation_commit(conn, id)?
        {
            return Ok(RemovalRun::Refused(Refusal::TypeBusy));
        }
        let plan = remove::plan(conn, &record, true)?;
        if plan.deletable == 0 {
            return Ok(RemovalRun::Nothing(plan));
        }
        let (outcome, trashed) = remove::remove(conn, plan)?;
        let value = serde_json::to_value(&outcome)
            .map_err(|e| diesel::result::Error::SerializationError(Box::new(e)))?;
        let updated = repo::record_removal(conn, id, auth.user_uuid, value)?;
        info!(
            id = %id,
            pages_trashed = outcome.deleted,
            pages_kept = outcome.kept.len(),
            collections_deleted = outcome.collections.deleted.len(),
            "moved imported pages to Trash"
        );
        Ok::<_, diesel::result::Error>(RemovalRun::Done(updated, trashed))
    });
    match result {
        Ok(RemovalRun::Done(job, trashed)) => {
            // The moves committed: the search index drops the pages now.
            for page in trashed {
                indexing_tasks::spawn_delete_documentation(search.get_ref().clone(), page);
            }
            respond(&mut tc, job, actix_web::http::StatusCode::OK)
        }
        Ok(RemovalRun::Refused(refusal)) => refusal.response(),
        Ok(RemovalRun::Nothing(plan)) => nothing_to_remove(&plan),
        Err(diesel::result::Error::NotFound) => not_found(id),
        Err(e) => {
            error!(id = %id, error = ?e, "failed to remove imported pages");
            errors::internal("Removing the imported pages failed and nothing was moved to Trash.")
        }
    }
}

/// `GET /admin/import/{id}/docs/publish`: `{publishable, left}`, the pages
/// Publish all would publish and those changed since the import that it
/// leaves as they are.
pub async fn publish_preview(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    path: web::Path<Uuid>,
) -> HttpResponse {
    if !auth.is_workspace_admin() {
        return errors::forbidden(FORBIDDEN);
    }
    let id = path.into_inner();
    let job = match load(&mut tc, &storage, id).await {
        Ok(job) => job,
        Err(resp) => return resp,
    };
    let record = match committed_record(&job) {
        Ok(record) => record,
        Err(refusal) => return refusal.response(),
    };
    match tc.run(|conn| remove::publish_plan(conn, &record, false)) {
        Ok(plan) => HttpResponse::Ok().json(plan),
        Err(e) => {
            error!(id = %id, error = ?e, "failed to plan publishing imported pages");
            errors::internal("failed to check the imported pages")
        }
    }
}

/// `POST /admin/import/{id}/docs/publish`: publish the import's pages still
/// in draft that nobody has changed since, a status change only, and
/// answer `{published, left}`.
pub async fn publish_all(
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
    search: web::Data<Arc<SearchService>>,
    path: web::Path<Uuid>,
) -> HttpResponse {
    if !auth.is_workspace_admin() {
        return errors::forbidden(FORBIDDEN);
    }
    let id = path.into_inner();
    if let Err(resp) = load(&mut tc, &storage, id).await {
        return resp;
    }
    let result = tc.run(|conn| {
        if !repo::try_lock_job(conn, id)? {
            return Ok(Read::Refused(Refusal::Busy));
        }
        let job = repo::get_for_update(conn, id)?;
        let mut record = match committed_record(&job) {
            Ok(record) => record,
            Err(refusal) => return Ok(Read::Refused(refusal)),
        };
        let plan = remove::publish_plan(conn, &record, true)?;
        let pages = remove::publish(conn, &mut record, &plan)?;
        if !pages.is_empty() {
            repo::update(
                conn,
                id,
                ImportJobUpdate {
                    record_ids: Some(serde_json::to_value(&record).ok()),
                    ..Default::default()
                },
            )?;
            info!(id = %id, pages_published = pages.len(), pages_left = plan.left, "published imported pages");
        }
        Ok::<_, diesel::result::Error>(Read::Ok((pages, plan.left)))
    });
    match result {
        Ok(Read::Ok((pages, left))) => {
            let published = pages.len();
            for page in pages {
                indexing_tasks::spawn_index_documentation(search.get_ref().clone(), page);
            }
            HttpResponse::Ok().json(serde_json::json!({
                "published": published,
                "left": left,
            }))
        }
        Ok(Read::Refused(refusal)) => refusal.response(),
        Err(diesel::result::Error::NotFound) => not_found(id),
        Err(e) => {
            error!(id = %id, error = ?e, "failed to publish imported pages");
            errors::internal("Publishing the imported pages failed and nothing was published.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loose_file_names_are_safe_and_unique() {
        let mut taken = std::collections::HashSet::new();
        assert_eq!(entry_name("notes/VPN.md", &mut taken), "VPN.md");
        assert_eq!(entry_name("C:\\docs\\vpn.md", &mut taken), "vpn (2).md");
        assert_eq!(entry_name("../../.hidden.md", &mut taken), "hidden.md");
        assert_eq!(entry_name("", &mut taken), "page.md");
        assert!(is_markdown_name("Read Me.MD"));
        assert!(!is_markdown_name("export.zip"));
    }
}
