//! The commit: a checked documentation import written as pages, in the
//! background.
//!
//! [`start`] runs in the request: under the job's lock it checks the plan
//! still holds (the target collection exists, the same pages are found in
//! the workspace already) and moves the job to `committing`, or answers
//! that the plan changed. [`spawn`] then:
//!
//! 1. reads the upload again and converts it under the same options; a
//!    plan that differs from the reviewed one stops here;
//! 2. mints each new page's uuid and the storage key of every image it
//!    shows, records them all on the job, then stores the images under
//!    `collab/doc/{page_uuid}/` (archive files, `data:` images and the web
//!    images the check downloaded, through the editor upload's checks);
//! 3. in one transaction as the admin: creates the collections, then the
//!    pages parents first (drafts unless chosen otherwise, never public,
//!    appended after existing siblings in an existing collection), then
//!    each body with links to the new pages' ids, embeds by uuid and image
//!    sources to the stored files, its embeds and its first revision
//!    ("Imported from {file}"), and the job's `done` with what was written;
//! 4. indexes the pages for search.
//!
//! A failure rolls the transaction back, deletes the stored images and puts
//! the job back on Review with "Nothing was imported. {reason}". A commit
//! whose process died is put back by [`super::reconcile`].

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use uuid::Uuid;

use super::background::{db, Heartbeat, Progress};
use super::check::{self, effective_mode, find_duplicates, outcome_of, Failure};
use super::images::{self, MAX_IMAGE_BYTES};
use super::{
    nothing_imported, parents_first, CommitRecord, DocsOptions, DocsRows, DocsSource, DocsSummary,
    Duplicates, ImageKind, PageOutcome, PageRow, RecordedPage, TenantWorkspace, WebImages,
    JOB_TYPE,
};
use crate::extractors::TenantConn;
use crate::models::{
    DocumentationPage, DocumentationStatus, ImportJob, ImportJobUpdate, NewDocumentationCollection,
    NewDocumentationCollectionPage, NewDocumentationPage,
};
use crate::repository::documentation_imports as docs_repo;
use crate::repository::imports as repo;
use crate::services::docs_markdown::encode;
use crate::services::docs_markdown::ir::Document;
use crate::services::docs_markdown::links::{fill_placeholders, Placeholder, Slot};
use crate::services::docs_markdown::tree::{CollectionMode, PageStatus, Plan};
use crate::services::search::{indexing_tasks, SearchService};
use crate::services::uploaded_archive::SafeArchive;
use crate::utils::storage::{Storage, StorageError};

/// The image source that drops an image to its alt text when filled.
const DROPPED: &str = "nosdesk-import:asset/dropped";

/// What starting a commit came to.
pub enum Start {
    Started(ImportJob),
    /// Committed already; the job as it is.
    Done(ImportJob),
    /// The plan the admin reviewed no longer holds; the job holds the new
    /// one.
    PlanChanged(ImportJob, String),
    /// Refused for now, with a code.
    Busy(&'static str, &'static str),
    /// The job can't be committed in its state.
    NotReady(String),
}

/// Why a commit can't start: the job isn't a checked documentation import.
pub fn not_ready(job: &ImportJob) -> Option<String> {
    if job.job_type != JOB_TYPE {
        return Some("this isn't a documentation import".to_string());
    }
    match job.status.as_str() {
        "dry_run_done" => None,
        "parsed" | "checking" => {
            Some("this import hasn't been checked yet; check the pages first".to_string())
        }
        status => Some(format!(
            "job is in status '{status}'; import requires a checked import"
        )),
    }
}

/// In the request, under the job's lock: check the plan still holds and
/// move the job to `committing` for run `run`. `choice` is the admin's
/// answer for pages found in the workspace already, when Review sends it.
pub fn start(
    conn: &mut crate::db::DbConnection,
    id: Uuid,
    run: Uuid,
    choice: Option<Duplicates>,
    expired: impl Fn(&ImportJob) -> Option<String>,
) -> diesel::QueryResult<Start> {
    if !repo::try_lock_job(conn, id)? {
        return Ok(Start::Busy(
            "This import is being changed. Try again in a moment.",
            "import_job_busy",
        ));
    }
    let job = repo::get_for_update(conn, id)?;
    if job.job_type == JOB_TYPE && job.status == "done" {
        return Ok(Start::Done(job));
    }
    if job.job_type == JOB_TYPE && job.status == "committing" {
        return Ok(Start::Busy(
            "This import is already being committed.",
            "import_commit_running",
        ));
    }
    if let Some(reason) = not_ready(&job).or_else(|| expired(&job)) {
        return Ok(Start::NotReady(reason));
    }
    if !repo::try_lock_type(conn, job.workspace_id, JOB_TYPE)?
        || repo::other_documentation_commit(conn, id)?
    {
        return Ok(Start::Busy(
            "Another documentation import is being committed. Try again when it finishes.",
            "import_type_busy",
        ));
    }
    let (Some(mut rows), Some(mut summary)) = (
        DocsRows::from_value(repo::row_results(conn, id)?),
        DocsSummary::from_job(&job),
    ) else {
        return Ok(Start::NotReady(
            "this import's check is gone; check the pages again".to_string(),
        ));
    };
    let mut options = DocsOptions::from_job(&job);
    if let Some(choice) = choice {
        options.duplicates = choice;
        summary.duplicates_choice = choice;
    }

    // The collection to import into still exists, with the page to go
    // under in it.
    let mode = summary.destination.mode;
    let mut changed: Option<String> = None;
    if mode == CollectionMode::Existing {
        let target = options.collection_id;
        let collection_ok = match target {
            Some(id) => docs_repo::collection_exists(conn, id)?,
            None => false,
        };
        let under_ok = match options.under_page_id {
            Some(page) => docs_repo::pages_by_id(conn, &[page])?
                .first()
                .is_some_and(|p| p.collection_id == target),
            None => true,
        };
        if !collection_ok || !under_ok {
            changed = Some(
                "Something changed since you checked: the collection or page to import into no longer exists. Choose where the pages go and check again."
                    .to_string(),
            );
        }
    }

    // The same pages are found in the workspace already.
    let keys: Vec<(usize, String, Option<String>)> = rows
        .pages
        .iter()
        .map(|p| (p.index, p.path.clone(), p.source_uuid.clone()))
        .collect();
    let fresh = find_duplicates(conn, &keys, &options, mode)?;
    let before = rows.pages.iter().filter(|p| p.duplicate.is_some()).count();
    let mut moved = false;
    for page in &mut rows.pages {
        let now = fresh.get(&page.index).cloned();
        if now.as_ref().map(|d| d.page_id) != page.duplicate.as_ref().map(|d| d.page_id) {
            moved = true;
        }
        page.duplicate = now;
        page.outcome = outcome_of(
            page.duplicate.as_ref(),
            page.error.is_some(),
            options.duplicates,
        );
    }
    restate(&mut summary, &rows.pages);
    if moved && changed.is_none() {
        let after = fresh.len();
        changed = Some(match after.cmp(&before) {
            std::cmp::Ordering::Greater => format!(
                "Something changed since you checked: {} more {} already in Nosdesk. Review the new plan.",
                after - before,
                if after - before == 1 { "page is" } else { "pages are" }
            ),
            _ => "Something changed since you checked: the pages already in Nosdesk are different. Review the new plan.".to_string(),
        });
    }
    if let Some(message) = changed {
        let updated = repo::update(
            conn,
            id,
            ImportJobUpdate {
                options: Some(serde_json::to_value(&options).ok()),
                summary: Some(serde_json::to_value(&summary).ok()),
                row_results: Some(serde_json::to_value(&rows).ok()),
                error_message: Some(Some(message.clone())),
                ..Default::default()
            },
        )?;
        return Ok(Start::PlanChanged(updated, message));
    }

    let record = CommitRecord {
        kind: JOB_TYPE.to_string(),
        run,
        pending: true,
        ..CommitRecord::default()
    };
    let updated = repo::update(
        conn,
        id,
        ImportJobUpdate {
            status: Some("committing".to_string()),
            options: Some(serde_json::to_value(&options).ok()),
            summary: Some(serde_json::to_value(&summary).ok()),
            row_results: Some(serde_json::to_value(&rows).ok()),
            record_ids: Some(serde_json::to_value(&record).ok()),
            records_committed: Some(Some(0)),
            error_message: Some(None),
            ..Default::default()
        },
    )?;
    Ok(Start::Started(updated))
}

/// The summary's counts after the rows' outcomes changed.
fn restate(summary: &mut DocsSummary, pages: &[PageRow]) {
    let skipped = pages
        .iter()
        .filter(|p| p.outcome == PageOutcome::Skipped)
        .count();
    summary.skipped = skipped;
    summary.rows_with_errors = skipped;
    summary.would_create = pages.len() - skipped;
    summary.duplicates = pages.iter().filter(|p| p.duplicate.is_some()).count();
    summary.notes = super::note_lines(pages);
}

/// What a commit needs, captured from the request that started it.
pub struct CommitRun {
    pub tc: TenantConn,
    pub storage: Arc<dyn Storage>,
    pub search: Arc<SearchService>,
    pub job_id: Uuid,
    pub run: Uuid,
    pub actor: Uuid,
    pub workspace_uuid: Uuid,
    pub filename: String,
}

/// Start the commit in the background.
pub fn spawn(run: CommitRun) {
    tokio::spawn(async move {
        let progress = Progress::new();
        let heartbeat = Heartbeat::start(
            run.tc.clone(),
            run.job_id,
            "committing",
            Some(progress.clone()),
        );
        let mut stored: Vec<String> = Vec::new();
        let result = commit(&run, &progress, &mut stored).await;
        drop(heartbeat);
        match result {
            Ok(pages) => {
                tracing::info!(id = %run.job_id, count = pages.len(), "documentation import committed");
                for page in pages {
                    indexing_tasks::spawn_index_documentation(run.search.clone(), page);
                }
            }
            Err(failure) => {
                let reason = match &failure {
                    Failure::Refused(r) => r.message.clone(),
                    Failure::Internal(e) => {
                        tracing::error!(id = %run.job_id, error = %e, "documentation commit failed");
                        "The import failed before it finished.".to_string()
                    }
                };
                delete_keys(run.storage.as_ref(), &stored).await;
                if let Err(e) = back_to_review(&run.tc, run.job_id, run.run, &reason).await {
                    tracing::error!(id = %run.job_id, error = ?e, "failed to record a documentation commit failure");
                }
            }
        }
    });
}

/// Delete stored files, logging what fails: one left behind is only
/// storage.
pub async fn delete_keys(storage: &dyn Storage, keys: &[String]) {
    for key in keys {
        match storage.delete_file(key).await {
            Ok(()) | Err(StorageError::NotFound(_)) => {}
            Err(e) => tracing::warn!(error = ?e, "failed to delete an imported image"),
        }
    }
}

/// Put job `id` back on Review after its commit `run` failed, if it is
/// still that run's.
async fn back_to_review(
    tc: &TenantConn,
    id: Uuid,
    run: Uuid,
    reason: &str,
) -> Result<(), diesel::result::Error> {
    let message = nothing_imported(reason);
    db(tc, move |conn| {
        let job = repo::get_for_update(conn, id)?;
        if !is_run(&job, run) {
            return Ok(());
        }
        repo::update(
            conn,
            id,
            ImportJobUpdate {
                status: Some("dry_run_done".to_string()),
                error_message: Some(Some(message)),
                record_ids: Some(None),
                records_committed: Some(None),
                ..Default::default()
            },
        )?;
        Ok(())
    })
    .await
}

/// Whether the job is committing for `run`.
fn is_run(job: &ImportJob, run: Uuid) -> bool {
    job.status == "committing"
        && CommitRecord::from_value(job.record_ids.clone()).is_some_and(|r| r.run == run)
}

/// Where an image a page shows comes from, to store it.
#[derive(Debug, Clone)]
enum Origin {
    Archive(String),
    Data(String),
    /// A file in this workspace's storage: a web image the check kept, or
    /// one a Nosdesk export of this workspace names.
    Stored(String),
}

/// One image to store with one page.
#[derive(Debug, Clone)]
struct ToStore {
    origin: Origin,
    key: String,
    name: String,
}

/// The images page `document` shows, each with the key it is stored under
/// in `folder`, and the source each image's current source becomes (`None`
/// when the image is dropped).
fn plan_images(
    document: &Document,
    row: &PageRow,
    rows: &DocsRows,
    web_choice: WebImages,
    folder: &str,
) -> (Vec<ToStore>, BTreeMap<String, Option<String>>) {
    let names: HashMap<&str, &str> = row
        .images
        .iter()
        .filter(|i| i.problem.is_none())
        .map(|i| (i.source.as_str(), i.name.as_str()))
        .collect();
    let mut to_store = Vec::new();
    let mut map = BTreeMap::new();
    let mut sources = Vec::new();
    images::for_each_image(document, &mut |image| sources.push(image.src.clone()));
    for src in sources {
        if map.contains_key(&src) {
            continue;
        }
        let label = images::source_label(&src);
        let origin = match images::kind_of(&src) {
            Some(ImageKind::Archive) => match Placeholder::parse(&src) {
                Some(Placeholder::Asset(key)) => Some(Origin::Archive(key)),
                _ => None,
            },
            Some(ImageKind::Data) => Some(Origin::Data(src.clone())),
            Some(ImageKind::Web) if web_choice == WebImages::Links => {
                map.insert(src.clone(), Some(src.clone()));
                continue;
            }
            Some(ImageKind::Web) => rows
                .web_images
                .get(&src)
                .and_then(|w| w.key.clone())
                .map(Origin::Stored),
            Some(ImageKind::Workspace) => images::workspace_image_key(&src).map(Origin::Stored),
            None => {
                // Not an image source the import knows: never written.
                map.insert(src.clone(), None);
                continue;
            }
        };
        // The check found a problem with it, or it can't be had: dropped.
        let checked_ok = names.contains_key(label.as_str());
        let Some(origin) = origin.filter(|_| checked_ok) else {
            map.insert(src.clone(), None);
            continue;
        };
        let name = names
            .get(label.as_str())
            .map(|n| n.to_string())
            .unwrap_or_else(|| images::name_of(&src, None));
        let key = format!("{folder}/{}_{name}", Uuid::now_v7());
        map.insert(src.clone(), Some(images::served_src(&key)));
        to_store.push(ToStore { origin, key, name });
    }
    (to_store, map)
}

/// The bytes of one image to store, checked as the editor's upload
/// checks them; `None` when it can't be had.
async fn image_bytes(
    item: &ToStore,
    archive: &Arc<Mutex<SafeArchive>>,
    storage: &dyn Storage,
) -> Option<(Vec<u8>, String)> {
    let bytes = match &item.origin {
        Origin::Archive(key) => {
            let archive = archive.clone();
            let key = key.clone();
            tokio::task::spawn_blocking(move || {
                archive
                    .lock()
                    .ok()
                    .and_then(|mut a| a.read(&key, MAX_IMAGE_BYTES).ok())
            })
            .await
            .ok()
            .flatten()?
        }
        Origin::Data(src) => images::decode_data(src).ok()?,
        Origin::Stored(key) => storage.get_file(key).await.ok()?,
    };
    let mime = images::validate(&bytes, &item.name).ok()?;
    Some((bytes, mime))
}

async fn commit(
    run: &CommitRun,
    progress: &Arc<Progress>,
    stored: &mut Vec<String>,
) -> Result<Vec<DocumentationPage>, Failure> {
    let id = run.job_id;
    let token = run.run;
    let job = db(&run.tc, move |conn| repo::get(conn, id))
        .await
        .map_err(|e| Failure::Internal(format!("read the job: {e}")))?;
    let options = DocsOptions::from_job(&job);
    let rows = DocsRows::from_value(
        db(&run.tc, move |conn| repo::row_results(conn, id))
            .await
            .map_err(|e| Failure::Internal(format!("read the check: {e}")))?,
    )
    .ok_or_else(|| Failure::Internal("the check's results are gone".to_string()))?;

    // The same plan, from the same file and options.
    let opened = check::open_upload(run.storage.as_ref(), id).await?;
    let workspace = TenantWorkspace::new(run.tc.clone(), run.workspace_uuid);
    let (built, archive) = check::build(
        opened.archive,
        options.tree_options(
            &run.filename,
            DocsSource::from_job(&job).map(|s| s.tree).as_ref(),
        ),
        workspace,
    )
    .await?;
    let plan = built.plan;
    let mut documents = built.documents;
    if !same_plan(&plan, &rows) {
        return Err(Failure::Refused(
            crate::services::imports::refusal::Refused::new(
                "import_plan_changed",
                "Something changed since you checked. Check the pages again.",
            ),
        ));
    }

    // Each new page's uuid and the images it stores.
    let mut uuids: Vec<Option<Uuid>> = vec![None; plan.pages.len()];
    let mut to_store: Vec<(usize, ToStore)> = Vec::new();
    for row in &rows.pages {
        if row.outcome == PageOutcome::Skipped {
            continue;
        }
        let uuid = Uuid::now_v7();
        uuids[row.index] = Some(uuid);
        let folder = format!("collab/doc/{uuid}");
        let (store, map) = plan_images(
            &documents[row.index],
            row,
            &rows,
            options.web_images,
            &folder,
        );
        images::for_each_image_mut(&mut documents[row.index], &mut |image| {
            image.src = match map.get(&image.src).cloned().flatten() {
                Some(src) => src,
                None => DROPPED.to_string(),
            };
        });
        to_store.extend(store.into_iter().map(|s| (row.index, s)));
    }

    // Recorded before anything is stored, so a failure or a process that
    // dies can remove them.
    let record = CommitRecord {
        kind: JOB_TYPE.to_string(),
        run: token,
        pending: true,
        page_uuids: uuids.iter().flatten().copied().collect(),
        storage_keys: to_store.iter().map(|(_, s)| s.key.clone()).collect(),
        ..CommitRecord::default()
    };
    let pending = serde_json::to_value(&record).ok();
    let ours = db(&run.tc, move |conn| {
        let job = repo::get_for_update(conn, id)?;
        if !is_run(&job, token) {
            return Ok(false);
        }
        repo::update(
            conn,
            id,
            ImportJobUpdate {
                record_ids: Some(pending),
                ..Default::default()
            },
        )?;
        Ok(true)
    })
    .await
    .map_err(|e| Failure::Internal(format!("record the import's files: {e}")))?;
    if !ours {
        return Err(Failure::Internal("the job was taken back".to_string()));
    }

    // Store the images. One that can't be read is dropped to its alt text
    // (the check reported it); one that can't be written fails the import.
    let archive = Arc::new(Mutex::new(archive));
    let mut dropped: Vec<(usize, String)> = Vec::new();
    for (index, item) in &to_store {
        match image_bytes(item, &archive, run.storage.as_ref()).await {
            Some((bytes, mime)) => {
                run.storage
                    .put_file(&bytes, &item.key, &mime)
                    .await
                    .map_err(|e| Failure::Internal(format!("store an image: {e:?}")))?;
                stored.push(item.key.clone());
            }
            None => dropped.push((*index, images::served_src(&item.key))),
        }
    }
    drop(archive);
    drop(opened._spooled);
    for (index, src) in dropped {
        images::for_each_image_mut(&mut documents[index], &mut |image| {
            if image.src == src {
                image.src = DROPPED.to_string();
            }
        });
    }

    // One transaction.
    let tc = run.tc.clone();
    let write = Write {
        plan,
        documents,
        rows,
        options,
        uuids,
        record,
        actor: run.actor,
        summary_text: format!("Imported from {}", run.filename),
        progress: progress.clone(),
    };
    let result = tokio::task::spawn_blocking(move || {
        let mut tc = tc;
        tc.run_result(|conn| write.apply(conn, id, token))
    })
    .await
    .map_err(|e| Failure::Internal(format!("write task: {e}")))?;
    match result {
        Ok(pages) => Ok(pages),
        Err(WriteError::Abandoned) => Err(Failure::Internal("the job was taken back".to_string())),
        Err(WriteError::Db(e)) => Err(Failure::Internal(format!("write the pages: {e}"))),
    }
}

/// Whether a plan built again is the one reviewed: the same pages, titles,
/// files and parents.
fn same_plan(plan: &Plan, rows: &DocsRows) -> bool {
    plan.pages.len() == rows.pages.len()
        && plan.pages.iter().zip(&rows.pages).all(|(p, r)| {
            p.index == r.index
                && p.title == r.title
                && p.parent == r.parent
                && p.source
                    .clone()
                    .or_else(|| p.folder.clone())
                    .unwrap_or_default()
                    == r.path
        })
}

#[derive(Debug)]
enum WriteError {
    Db(diesel::result::Error),
    /// The job isn't this run's any more.
    Abandoned,
}

impl From<diesel::result::Error> for WriteError {
    fn from(e: diesel::result::Error) -> Self {
        WriteError::Db(e)
    }
}

/// Everything the transaction writes.
struct Write {
    plan: Plan,
    documents: Vec<Document>,
    rows: DocsRows,
    options: DocsOptions,
    uuids: Vec<Option<Uuid>>,
    record: CommitRecord,
    actor: Uuid,
    summary_text: String,
    progress: Arc<Progress>,
}

/// A page written, for the record.
#[derive(Debug, Clone, Serialize)]
struct Written {
    id: i32,
    uuid: Uuid,
    collection_id: Option<i32>,
}

fn status_of(status: PageStatus) -> DocumentationStatus {
    match status {
        PageStatus::Draft => DocumentationStatus::Draft,
        PageStatus::Published => DocumentationStatus::Published,
        PageStatus::Archived => DocumentationStatus::Archived,
    }
}

/// At most `max` characters.
fn cut(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// The state vector of a Yjs update, for the revision.
fn state_vector_of(update: &[u8]) -> Vec<u8> {
    use yrs::updates::decoder::Decode;
    use yrs::updates::encoder::Encode;
    use yrs::{Doc, ReadTxn, Transact, Update};
    let doc = Doc::new();
    {
        let mut txn = doc.transact_mut();
        if let Ok(update) = Update::decode_v1(update) {
            let _ = txn.apply_update(update);
        }
    }
    let txn = doc.transact();
    txn.state_vector().encode_v1()
}

impl Write {
    fn apply(
        self,
        conn: &mut crate::db::DbConnection,
        job_id: Uuid,
        run: Uuid,
    ) -> Result<Vec<DocumentationPage>, WriteError> {
        use crate::repository;
        use crate::repository::documentation_collections as collections;
        use crate::utils::slug;

        let Write {
            plan,
            mut documents,
            rows,
            options,
            uuids,
            mut record,
            actor,
            summary_text,
            progress,
        } = self;
        let mode = effective_mode(&options, plan.profile);

        // Collections.
        let mut created_collections: Vec<i32> = Vec::new();
        let target = if mode == CollectionMode::Existing {
            options.collection_id
        } else {
            None
        };
        if mode != CollectionMode::Existing {
            for c in &plan.collections {
                let name = cut(c.name.trim(), 255);
                let name = if name.is_empty() {
                    "Imported documentation".to_string()
                } else {
                    name
                };
                let slug = c
                    .slug
                    .as_deref()
                    .and_then(|s| slug::normalize_unique_collection_slug(&cut(s, 200), conn))
                    .unwrap_or_else(|| {
                        slug::generate_unique_collection_slug(&cut(&name, 200), conn)
                    });
                let collection = collections::create_collection(
                    conn,
                    NewDocumentationCollection {
                        uuid: Uuid::now_v7(),
                        name,
                        slug,
                        description: c.description.clone(),
                        icon: c.icon.clone().filter(|i| i.chars().count() <= 50),
                        color: c
                            .color
                            .clone()
                            .filter(|c| c.len() <= 7 && c.starts_with('#')),
                        is_system: false,
                        created_by: Some(actor),
                    },
                )?;
                created_collections.push(collection.id);
            }
        }

        // Pages found already that a skipped page stands for, with their
        // collection: a child of a skipped page goes under it there.
        let skipped_ids: Vec<i32> = rows
            .pages
            .iter()
            .filter(|r| r.outcome == PageOutcome::Skipped)
            .filter_map(|r| r.duplicate.as_ref().map(|d| d.page_id))
            .collect();
        let skipped_at: HashMap<i32, Option<i32>> = docs_repo::pages_by_id(conn, &skipped_ids)?
            .into_iter()
            .map(|p| (p.id, p.collection_id))
            .collect();

        // Appended after what an existing collection holds already.
        let base_order = match target {
            Some(collection) => {
                docs_repo::last_sibling_order(conn, collection, options.under_page_id)?
                    .map_or(0, |o| o + 1)
            }
            None => 0,
        };

        // Pages, parents first.
        let mut written: Vec<Option<Written>> = vec![None; plan.pages.len()];
        for index in parents_first(&plan) {
            let page = &plan.pages[index];
            let Some(uuid) = uuids[index] else {
                continue;
            };
            let planned_collection = match target {
                Some(t) => Some(t),
                None => page
                    .collection
                    .and_then(|c| created_collections.get(c).copied()),
            };
            let (parent_id, collection_id, display_order) = match page.parent {
                Some(p) => match &written[p] {
                    Some(parent) => (Some(parent.id), parent.collection_id, page.order as i32),
                    None => {
                        // The parent was skipped: under the page it matched
                        // when that is where this page goes, else at the top.
                        let matched = rows.pages[p].duplicate.as_ref().map(|d| d.page_id);
                        match matched.filter(|m| {
                            skipped_at.get(m).copied().flatten() == planned_collection
                                && planned_collection.is_some()
                        }) {
                            Some(m) => (Some(m), planned_collection, page.order as i32),
                            None => (
                                if target.is_some() {
                                    options.under_page_id
                                } else {
                                    None
                                },
                                planned_collection,
                                base_order + page.order as i32,
                            ),
                        }
                    }
                },
                None => (
                    if target.is_some() {
                        options.under_page_id
                    } else {
                        None
                    },
                    planned_collection,
                    base_order + page.order as i32,
                ),
            };
            let title = cut(&page.title, 255);
            let kept_slug = page
                .slug
                .as_deref()
                .map(slug::slugify)
                .filter(|s| !s.is_empty() && s.len() <= 200)
                .filter(|s| repository::get_documentation_page_by_slug(s, conn).is_err());
            let page_slug =
                kept_slug.unwrap_or_else(|| slug::generate_unique_slug(&cut(&title, 200), conn));
            let created = repository::create_documentation_page(
                NewDocumentationPage {
                    uuid,
                    title,
                    slug: page_slug,
                    icon: page.icon.clone(),
                    cover_image: None,
                    status: status_of(page.status),
                    created_by: actor,
                    last_edited_by: actor,
                    parent_id,
                    display_order: Some(display_order),
                    is_public: false,
                    is_template: false,
                    yjs_state_vector: None,
                    yjs_document: None,
                    yjs_client_id: None,
                    has_unsaved_changes: false,
                },
                conn,
            )?;
            if let Some(collection_id) = collection_id {
                collections::add_page_to_collection(
                    conn,
                    NewDocumentationCollectionPage {
                        collection_id,
                        page_id: created.id,
                        created_by: Some(actor),
                    },
                )?;
            }
            written[index] = Some(Written {
                id: created.id,
                uuid,
                collection_id,
            });
        }

        // Where a page in the import is now: one written, or the page a
        // skipped one matched.
        let target_of = |j: usize| -> Option<(i32, Uuid)> {
            if let Some(w) = written.get(j).and_then(|w| w.as_ref()) {
                return Some((w.id, w.uuid));
            }
            rows.pages
                .get(j)
                .and_then(|r| r.duplicate.as_ref())
                .map(|d| (d.page_id, d.page_uuid))
        };
        let fill = |p: &Placeholder, slot: Slot| -> Option<String> {
            match (p, slot) {
                (Placeholder::Page(j), Slot::Link) => {
                    target_of(*j).map(|(id, _)| format!("/documentation/{id}"))
                }
                (Placeholder::Page(j), Slot::Embed) => {
                    target_of(*j).map(|(_, uuid)| uuid.to_string())
                }
                _ => None,
            }
        };
        let by_uuid: HashMap<Uuid, i32> =
            written.iter().flatten().map(|w| (w.uuid, w.id)).collect();

        // Bodies.
        let mut pages_out = Vec::new();
        for index in parents_first(&plan) {
            let Some(w) = written[index].clone() else {
                continue;
            };
            let document = &mut documents[index];
            fill_placeholders(document, &fill);
            // Never a `data:` source, whatever happened above.
            images::for_each_image_mut(document, &mut |image| {
                if images::is_data(&image.src) || Placeholder::parse(&image.src).is_some() {
                    image.src = DROPPED.to_string();
                }
            });
            fill_placeholders(document, &fill);
            let update = encode::encode(document);
            let page =
                repository::update_documentation_yjs_state(conn, w.id, update.clone(), None, None)?;

            let embedded: Vec<Uuid> = images::embedded_uuids(document)
                .iter()
                .filter_map(|u| Uuid::parse_str(u).ok())
                .collect();
            let mut targets: Vec<i32> = embedded
                .iter()
                .filter_map(|u| by_uuid.get(u).copied())
                .collect();
            let elsewhere: Vec<Uuid> = embedded
                .iter()
                .filter(|u| !by_uuid.contains_key(u))
                .copied()
                .collect();
            targets.extend(
                docs_repo::pages_by_uuid(conn, &elsewhere)?
                    .iter()
                    .map(|p| p.id),
            );
            targets.sort_unstable();
            targets.dedup();
            targets.retain(|t| *t != w.id);
            repository::sync_page_embeddings(conn, w.id, &targets)?;

            repository::create_revision_with_summary(
                conn,
                w.id,
                state_vector_of(&update),
                update.clone(),
                actor,
                &summary_text,
            )?;

            use sha2::{Digest, Sha256};
            record.pages.push(RecordedPage {
                id: w.id,
                uuid: w.uuid,
                index,
                source: rows.pages[index].path.clone(),
                collection_id: w.collection_id,
                updated_at: page.updated_at,
                hash: hex::encode(Sha256::digest(&update)),
            });
            progress.page_written();
            pages_out.push(page);
        }

        // The job, last, in the same transaction: written pages and `done`
        // commit together.
        let job = repo::get_for_update(conn, job_id)?;
        if !is_run(&job, run) {
            return Err(WriteError::Abandoned);
        }
        record.pending = false;
        record.collections_created = created_collections;
        record.pages.sort_by_key(|p| p.index);
        repo::update(
            conn,
            job_id,
            ImportJobUpdate {
                status: Some("done".to_string()),
                completed_at: Some(Some(chrono::Utc::now())),
                records_committed: Some(Some(pages_out.len() as i32)),
                record_ids: Some(serde_json::to_value(&record).ok()),
                error_message: Some(None),
                ..Default::default()
            },
        )?;
        Ok(pages_out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::docs_markdown::ir::{Block, Image, Inline};

    #[test]
    fn a_state_vector_reads_from_an_update() {
        let document = Document {
            blocks: vec![Block::Paragraph(vec![Inline::text("Hello")])],
        };
        let update = encode::encode(&document);
        assert!(!state_vector_of(&update).is_empty());
    }

    #[test]
    fn dropped_images_keep_their_alt_text() {
        let mut document = Document {
            blocks: vec![Block::Paragraph(vec![
                Inline::text("Before "),
                Inline::Image(Image {
                    src: DROPPED.to_string(),
                    alt: Some("diagram".into()),
                    ..Image::default()
                }),
            ])],
        };
        fill_placeholders(&mut document, &|_, _| None);
        assert_eq!(
            document.blocks[0],
            Block::Paragraph(vec![Inline::text("Before diagram")])
        );
    }
}
