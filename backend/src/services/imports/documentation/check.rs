//! The check: the dry run of a documentation import, in the background.
//!
//! Reads the upload under the admin's options, converts every page, reads
//! and validates every image (downloading web images unless they are kept
//! as links), finds the pages imported before, writes each page's preview
//! and lands the job in `dry_run_done` with the summary and every page's
//! result. Nothing is written to documentation tables. A file that can't be
//! imported under these options puts the job back in `parsed` with the
//! reason.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use futures::StreamExt;
use uuid::Uuid;

use super::background::{db, Heartbeat};
use super::images::{self, MAX_IMAGE_BYTES};
use super::preview;
use super::web::{self, WebFetcher};
use super::{
    note_lines, preview_key, upload_key, CommitRecord, Destination, DocsOptions, DocsRows,
    DocsSummary, Duplicate, Duplicates, FoundBy, ImageKind, ImageProblem, PageImage, PageOutcome,
    PageRow, TenantWorkspace, TreeView, WebImage, WebImages, CONVERT_BUDGET, JOB_TYPE,
};
use crate::extractors::TenantConn;
use crate::models::ImportJobUpdate;
use crate::repository::documentation_imports as docs_repo;
use crate::repository::imports as repo;
use crate::services::docs_markdown::ir::Document;
use crate::services::docs_markdown::links::Placeholder;
use crate::services::docs_markdown::profiles::ProfileKind;
use crate::services::docs_markdown::tree::{self, Built, CollectionMode, Plan};
use crate::services::imports::refusal::Refused;
use crate::services::uploaded_archive::{spool, ArchiveError, ArchiveOptions, SafeArchive};
use crate::utils::storage::Storage;

/// What a check needs, captured from the request that started it.
pub struct CheckRun {
    pub tc: TenantConn,
    pub storage: Arc<dyn Storage>,
    pub job_id: Uuid,
    /// This run's token, in the job's summary while it checks.
    pub run: Uuid,
    pub workspace_uuid: Uuid,
    pub filename: String,
    pub options: DocsOptions,
    /// The tree the admin unticked pages in (the job's `source.tree`).
    pub shown: Option<TreeView>,
    /// An earlier check's web images, downloaded already.
    pub kept_web: BTreeMap<String, WebImage>,
}

/// The summary a job holds while a check runs: whose run it is.
pub fn running_summary(run: Uuid) -> serde_json::Value {
    serde_json::json!({ "kind": JOB_TYPE, "run": run })
}

/// The collection mode the tree uses for these options and profile.
pub fn effective_mode(options: &DocsOptions, profile: ProfileKind) -> CollectionMode {
    options.collections.unwrap_or(match profile {
        ProfileKind::NosdeskV2 => CollectionMode::AsSource,
        ProfileKind::Outline => CollectionMode::PerTopLevelFolder,
        _ => CollectionMode::New,
    })
}

/// Start the check in the background.
pub fn spawn(run: CheckRun) {
    tokio::spawn(async move {
        let heartbeat = Heartbeat::start(run.tc.clone(), run.job_id, "checking", None);
        let result = check(&run).await;
        drop(heartbeat);
        let job_id = run.job_id;
        let finished = match result {
            Ok(checked) => finish(&run, checked).await,
            Err(failure) => fail(&run, failure).await,
        };
        if let Err(e) = finished {
            tracing::error!(id = %job_id, error = ?e, "failed to record a documentation check");
        }
    });
}

/// Why a check ended without a plan.
#[derive(Debug)]
pub enum Failure {
    /// The file can't be imported under these options.
    Refused(Refused),
    /// Something on our side; the admin can check again.
    Internal(String),
}

impl From<Refused> for Failure {
    fn from(r: Refused) -> Self {
        Failure::Refused(r)
    }
}

/// What a finished check stores.
pub struct Checked {
    pub summary: DocsSummary,
    pub rows: DocsRows,
}

/// The upload, opened, with the temp file it reads from.
pub struct Opened {
    pub archive: SafeArchive,
    /// Removed when dropped.
    pub _spooled: spool::Spooled,
}

/// Copy the upload from storage to a temp file and open it.
pub async fn open_upload(storage: &dyn Storage, job_id: Uuid) -> Result<Opened, Failure> {
    let spooled = spool::fetch(storage, &upload_key(job_id), None)
        .await
        .map_err(|e| match e {
            crate::utils::storage::StorageError::NotFound(_) => Failure::Refused(Refused::new(
                crate::services::imports::files::MISSING_CODE,
                crate::services::imports::files::MISSING,
            )),
            e => Failure::Internal(format!("read the upload: {e:?}")),
        })?;
    let file = spooled
        .open()
        .map_err(|e| Failure::Internal(format!("open the upload: {e}")))?;
    let archive = tokio::task::spawn_blocking(move || {
        SafeArchive::open(file, &ArchiveOptions::documentation())
    })
    .await
    .map_err(|e| Failure::Internal(e.to_string()))?
    .map_err(|e| Failure::Refused(e.into()))?;
    Ok(Opened {
        archive,
        _spooled: spooled,
    })
}

/// Build the plan off the async worker, within [`CONVERT_BUDGET`]. The
/// archive comes back for reading images.
pub async fn build(
    mut archive: SafeArchive,
    options: tree::ImportOptions,
    workspace: TenantWorkspace,
) -> Result<(Built, SafeArchive), Failure> {
    let task = tokio::task::spawn_blocking(move || {
        let built = tree::build(&mut archive, &options, &workspace);
        (built, archive)
    });
    match tokio::time::timeout(CONVERT_BUDGET, task).await {
        Ok(Ok((Ok(built), archive))) => Ok((built, archive)),
        Ok(Ok((Err(refused), _))) => Err(Failure::Refused(refused)),
        Ok(Err(e)) => Err(Failure::Internal(format!("conversion task: {e}"))),
        Err(_) => Err(Failure::Refused(timed_out())),
    }
}

/// The conversion ran past its budget.
pub fn timed_out() -> Refused {
    Refused::new(
        "docs_import_timed_out",
        "Reading the pages took too long. Import a smaller part at a time.",
    )
}

/// Every image a page shows, as the check finds it, without web images'
/// downloads (filled in after). Kept web links are listed too.
fn page_images(
    archive: &mut SafeArchive,
    document: &Document,
    archive_cache: &mut HashMap<String, PageImage>,
) -> Vec<PageImage> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let mut sources = Vec::new();
    images::for_each_image(document, &mut |image| sources.push(image.src.clone()));
    for src in sources {
        let label = images::source_label(&src);
        if !seen.insert(label.clone()) {
            continue;
        }
        let Some(kind) = images::kind_of(&src) else {
            continue;
        };
        let found = match kind {
            ImageKind::Archive => {
                let Some(Placeholder::Asset(key)) = Placeholder::parse(&src) else {
                    continue;
                };
                archive_cache
                    .entry(key.clone())
                    .or_insert_with(|| read_archive_image(archive, &key))
                    .clone()
            }
            ImageKind::Data => {
                let checked = images::decode_data(&src).and_then(|bytes| {
                    let name = images::name_of(&src, None);
                    images::validate(&bytes, &name).map(|mime| (bytes.len() as u64, mime))
                });
                match checked {
                    Ok((size, mime)) => PageImage {
                        source: label.clone(),
                        kind,
                        name: images::name_of(&src, Some(&mime)),
                        size: Some(size),
                        problem: None,
                    },
                    Err(problem) => PageImage {
                        source: label.clone(),
                        kind,
                        name: images::name_of(&src, None),
                        size: None,
                        problem: Some(problem),
                    },
                }
            }
            ImageKind::Web | ImageKind::Workspace => PageImage {
                source: label.clone(),
                kind,
                name: images::name_of(&src, None),
                size: None,
                problem: None,
            },
        };
        out.push(found);
    }
    out
}

fn read_archive_image(archive: &mut SafeArchive, key: &str) -> PageImage {
    let src = Placeholder::Asset(key.to_string()).to_href();
    let name = images::name_of(&src, None);
    let read = archive.read(key, MAX_IMAGE_BYTES).map_err(|e| match e {
        ArchiveError::EntryTooLarge { .. } => ImageProblem::TooLarge,
        _ => ImageProblem::Unreadable,
    });
    let checked = read
        .and_then(|bytes| images::validate(&bytes, &name).map(|mime| (bytes.len() as u64, mime)));
    match checked {
        Ok((size, mime)) => PageImage {
            source: key.to_string(),
            kind: ImageKind::Archive,
            name: images::name_of(&src, Some(&mime)),
            size: Some(size),
            problem: None,
        },
        Err(problem) => PageImage {
            source: key.to_string(),
            kind: ImageKind::Archive,
            name,
            size: None,
            problem: Some(problem),
        },
    }
}

/// The pages of a plan the workspace holds already: by the Nosdesk uuid a
/// page carries, or, importing into an existing collection, by the same
/// file an earlier import brought into it.
pub fn find_duplicates(
    conn: &mut crate::db::DbConnection,
    pages: &[(usize, String, Option<String>)],
    options: &DocsOptions,
    mode: CollectionMode,
) -> diesel::QueryResult<BTreeMap<usize, Duplicate>> {
    let mut found = BTreeMap::new();
    let uuids: Vec<Uuid> = pages
        .iter()
        .filter_map(|(_, _, u)| u.as_deref().and_then(|u| Uuid::parse_str(u.trim()).ok()))
        .collect();
    let by_uuid: HashMap<Uuid, docs_repo::ExistingPage> = docs_repo::pages_by_uuid(conn, &uuids)?
        .into_iter()
        .map(|p| (p.uuid, p))
        .collect();
    for (index, _, uuid) in pages {
        let Some(page) = uuid
            .as_deref()
            .and_then(|u| Uuid::parse_str(u.trim()).ok())
            .and_then(|u| by_uuid.get(&u))
        else {
            continue;
        };
        found.insert(
            *index,
            Duplicate {
                page_id: page.id,
                page_uuid: page.uuid,
                by: FoundBy::Uuid,
            },
        );
    }

    let target = match (mode, options.collection_id) {
        (CollectionMode::Existing, Some(id)) => id,
        _ => return Ok(found),
    };
    // The newest import of a file wins.
    let mut earlier: HashMap<String, i32> = HashMap::new();
    for value in repo::documentation_records(conn)? {
        let Some(record) = CommitRecord::from_value(Some(value)) else {
            continue;
        };
        for page in record.pages {
            if page.collection_id == Some(target) {
                earlier.insert(page.source, page.id);
            }
        }
    }
    let ids: Vec<i32> = pages
        .iter()
        .filter(|(index, _, _)| !found.contains_key(index))
        .filter_map(|(_, path, _)| earlier.get(path).copied())
        .collect();
    let still: HashMap<i32, docs_repo::ExistingPage> = docs_repo::pages_by_id(conn, &ids)?
        .into_iter()
        .filter(|p| p.collection_id == Some(target))
        .map(|p| (p.id, p))
        .collect();
    for (index, path, _) in pages {
        if found.contains_key(index) {
            continue;
        }
        if let Some(page) = earlier.get(path).and_then(|id| still.get(id)) {
            found.insert(
                *index,
                Duplicate {
                    page_id: page.id,
                    page_uuid: page.uuid,
                    by: FoundBy::Source,
                },
            );
        }
    }
    Ok(found)
}

/// The pages' keys for [`find_duplicates`]: index, path and uuid.
pub fn duplicate_keys(plan: &Plan) -> Vec<(usize, String, Option<String>)> {
    plan.pages
        .iter()
        .map(|p| {
            (
                p.index,
                p.source
                    .clone()
                    .or_else(|| p.folder.clone())
                    .unwrap_or_default(),
                p.uuid.clone(),
            )
        })
        .collect()
}

/// A page's outcome given what it matched and the admin's choice.
pub fn outcome_of(duplicate: Option<&Duplicate>, error: bool, choice: Duplicates) -> PageOutcome {
    match (duplicate, choice) {
        (Some(_), Duplicates::Skip) => PageOutcome::Skipped,
        _ if error => PageOutcome::Error,
        _ => PageOutcome::New,
    }
}

async fn check(run: &CheckRun) -> Result<Checked, Failure> {
    let opened = open_upload(run.storage.as_ref(), run.job_id).await?;
    let workspace = TenantWorkspace::new(run.tc.clone(), run.workspace_uuid);
    let (built, archive) = build(
        opened.archive,
        run.options.tree_options(&run.filename, run.shown.as_ref()),
        workspace,
    )
    .await?;

    // Archive and `data:` images, read and validated off the worker.
    let web_choice = run.options.web_images;
    let documents = built.documents;
    let (documents, mut found) = tokio::task::spawn_blocking(move || {
        let mut archive = archive;
        let mut cache = HashMap::new();
        let found: Vec<Vec<PageImage>> = documents
            .iter()
            .map(|d| page_images(&mut archive, d, &mut cache))
            .collect();
        (documents, found)
    })
    .await
    .map_err(|e| Failure::Internal(format!("image check: {e}")))?;
    drop(opened._spooled);

    // Web images, downloaded once each.
    let mut urls = Vec::new();
    let mut seen = BTreeSet::new();
    for page in &found {
        for image in page {
            if image.kind == ImageKind::Web && seen.insert(image.source.clone()) {
                urls.push(image.source.clone());
            }
        }
    }
    let web_images = if web_choice == WebImages::Download && !urls.is_empty() {
        let fetcher =
            WebFetcher::safe().map_err(|e| Failure::Internal(format!("web image client: {e}")))?;
        web::fetch_all(
            &fetcher,
            run.storage.clone(),
            run.job_id,
            urls,
            &run.kept_web,
        )
        .await
    } else {
        BTreeMap::new()
    };
    for page in &mut found {
        for image in page.iter_mut() {
            if let Some(web) = web_images.get(&image.source) {
                image.name = web.name.clone();
                image.size = web.size;
                image.problem = web.problem;
            }
        }
    }

    let plan = built.plan;
    let mode = effective_mode(&run.options, plan.profile);
    let options = run.options.clone();
    let keys = duplicate_keys(&plan);
    let (duplicates, collection_name) = db(&run.tc, move |conn| {
        let duplicates = find_duplicates(conn, &keys, &options, mode)?;
        let name = match options.collection_id {
            Some(id) => {
                Some(crate::repository::documentation_collections::get_collection(conn, id)?.name)
            }
            None => None,
        };
        Ok((duplicates, name))
    })
    .await
    .map_err(|e| match e {
        diesel::result::Error::NotFound => Failure::Refused(collection_gone()),
        e => Failure::Internal(format!("duplicate check: {e}")),
    })?;

    // Rows.
    let mut rows = Vec::with_capacity(plan.pages.len());
    for (page, images) in plan.pages.iter().zip(found.iter()) {
        let mut notes = page.notes.clone();
        for image in images {
            if let Some(problem) = image.problem {
                notes.add(problem.note());
            }
        }
        let duplicate = duplicates.get(&page.index).cloned();
        rows.push(PageRow {
            index: page.index,
            path: page
                .source
                .clone()
                .or_else(|| page.folder.clone())
                .unwrap_or_default(),
            title: page.title.clone(),
            parent: page.parent,
            outcome: outcome_of(
                duplicate.as_ref(),
                page.error.is_some(),
                run.options.duplicates,
            ),
            duplicate,
            source_uuid: page.uuid.clone(),
            notes,
            links: page.links.clone(),
            images: images.clone(),
            error: page.error.clone(),
        });
    }

    // Previews.
    let previews: Vec<(usize, Vec<u8>)> = plan
        .pages
        .iter()
        .zip(documents)
        .zip(found.iter())
        .map(|((page, document), images)| {
            let p = preview::preview(page.index, &page.title, document, images);
            (page.index, serde_json::to_vec(&p).unwrap_or_default())
        })
        .collect();
    let storage = run.storage.clone();
    let job_id = run.job_id;
    let written: Vec<bool> = futures::stream::iter(previews)
        .map(|(index, bytes)| {
            let storage = storage.clone();
            async move {
                storage
                    .put_file(&bytes, &preview_key(job_id, index), "application/json")
                    .await
                    .is_ok()
            }
        })
        .buffer_unordered(8)
        .collect()
        .await;
    if written.iter().any(|ok| !ok) {
        return Err(Failure::Internal("write the previews".to_string()));
    }

    let summary = summarise(&plan, &rows, &web_images, run, mode, collection_name);
    Ok(Checked {
        summary,
        rows: DocsRows {
            kind: JOB_TYPE.to_string(),
            pages: rows,
            web_images,
        },
    })
}

/// The target collection is gone.
pub fn collection_gone() -> Refused {
    Refused::new(
        "docs_import_collection_missing",
        "The collection to import into no longer exists. Choose another.",
    )
}

fn summarise(
    plan: &Plan,
    rows: &[PageRow],
    web_images: &BTreeMap<String, WebImage>,
    run: &CheckRun,
    mode: CollectionMode,
    collection_name: Option<String>,
) -> DocsSummary {
    let count = |o: PageOutcome| rows.iter().filter(|r| r.outcome == o).count();
    let skipped = count(PageOutcome::Skipped);
    let imported: Vec<&PageRow> = rows
        .iter()
        .filter(|r| r.outcome != PageOutcome::Skipped)
        .collect();
    let images = imported
        .iter()
        .flat_map(|r| r.images.iter())
        .filter(|i| i.problem.is_none())
        .filter(|i| !(i.kind == ImageKind::Web && run.options.web_images == WebImages::Links))
        .count();
    let image_problems = imported
        .iter()
        .flat_map(|r| r.images.iter())
        .filter(|i| i.problem.is_some())
        .count();
    DocsSummary {
        kind: JOB_TYPE.to_string(),
        row_count: rows.len(),
        would_create: rows.len() - skipped,
        would_update: 0,
        unchanged: 0,
        rows_with_errors: skipped,
        skipped,
        duplicates: rows.iter().filter(|r| r.duplicate.is_some()).count(),
        errors: count(PageOutcome::Error),
        images,
        image_problems,
        web_images: web_images.len(),
        links_rewritten: imported.iter().map(|r| r.links.rewritten).sum(),
        notes: note_lines(rows),
        section_pages: plan.section_pages,
        left_out: plan.left_out.len(),
        front_matter_not_imported: plan.front_matter_not_imported.clone(),
        profile: plan.profile,
        detected: plan.detected,
        destination: Destination {
            mode,
            new_collections: plan.collections.iter().map(|c| c.name.clone()).collect(),
            collection_id: run.options.collection_id,
            collection_name,
            under_page_id: run.options.under_page_id,
        },
        status: run.options.status,
        duplicates_choice: run.options.duplicates,
        web_images_choice: run.options.web_images,
    }
}

/// Whether the job is still this run's check, read under its row lock.
fn still_ours(
    conn: &mut crate::db::DbConnection,
    id: Uuid,
    run: Uuid,
) -> diesel::QueryResult<bool> {
    let job = repo::get_for_update(conn, id)?;
    Ok(job.status == "checking"
        && job
            .summary
            .as_ref()
            .and_then(|s| s.get("run"))
            .and_then(|r| r.as_str())
            == Some(run.to_string().as_str()))
}

async fn finish(run: &CheckRun, checked: Checked) -> Result<(), diesel::result::Error> {
    let id = run.job_id;
    let token = run.run;
    let summary = serde_json::to_value(&checked.summary).ok();
    let rows = serde_json::to_value(&checked.rows).ok();
    let written = db(&run.tc, move |conn| {
        if !still_ours(conn, id, token)? {
            return Ok(false);
        }
        repo::update(
            conn,
            id,
            ImportJobUpdate {
                status: Some("dry_run_done".to_string()),
                summary: Some(summary),
                row_results: Some(rows),
                error_message: Some(None),
                ..Default::default()
            },
        )?;
        Ok(true)
    })
    .await?;
    if written {
        tracing::info!(id = %id, count = checked.summary.row_count, "documentation import checked");
    }
    Ok(())
}

async fn fail(run: &CheckRun, failure: Failure) -> Result<(), diesel::result::Error> {
    let id = run.job_id;
    let token = run.run;
    let (message, refused) = match failure {
        Failure::Refused(r) => (r.message.clone(), Some(r)),
        Failure::Internal(e) => {
            tracing::error!(id = %id, error = %e, "documentation check failed");
            (
                "The check couldn't finish. Check the pages again.".to_string(),
                None,
            )
        }
    };
    let summary = refused.map(|r| {
        serde_json::json!({
            "kind": JOB_TYPE,
            "refused": { "code": r.code, "message": r.message, "args": r.args },
        })
    });
    db(&run.tc, move |conn| {
        if !still_ours(conn, id, token)? {
            return Ok(());
        }
        repo::update(
            conn,
            id,
            ImportJobUpdate {
                status: Some("parsed".to_string()),
                summary: Some(summary),
                row_results: Some(None),
                error_message: Some(Some(message)),
                ..Default::default()
            },
        )?;
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duplicate_skips_only_when_asked() {
        let dup = Duplicate {
            page_id: 4,
            page_uuid: Uuid::now_v7(),
            by: FoundBy::Uuid,
        };
        assert_eq!(
            outcome_of(Some(&dup), false, Duplicates::Skip),
            PageOutcome::Skipped
        );
        assert_eq!(
            outcome_of(Some(&dup), false, Duplicates::Copy),
            PageOutcome::New
        );
        assert_eq!(outcome_of(None, true, Duplicates::Skip), PageOutcome::Error);
        assert_eq!(outcome_of(None, false, Duplicates::Skip), PageOutcome::New);
    }
}
