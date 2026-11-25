//! Incremental search index updates, off the request path.
//!
//! Every write goes on one queue per index, drained by one thread: a write
//! may read the database (an asset's records, a user's memberships) and
//! takes the index writer's lock, so it belongs on neither an async worker
//! nor a pool of threads racing each other for that lock. The consumer takes
//! what has queued up, collapses it to one operation per document (the last
//! one queued, a write or a delete, so a page archived then restored ends up
//! back in the index), applies them in arrival order and commits once per
//! batch. A 5,000-row import is then one or a few batches, not 5,000
//! commits. A job that panics is logged and the batch carries on.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, Weak};
use tracing::{debug, error};

use super::{EntityType, SearchService};
use crate::models;

type BoxError = Box<dyn std::error::Error + Send + Sync>;
type IndexFn = Box<dyn FnOnce(&SearchService) -> Result<(), BoxError> + Send>;

/// The most queued writes one batch takes before it commits.
const BATCH_MAX: usize = 1_000;

/// What to do to one document.
pub(super) enum Op<T> {
    /// Write it: `T` builds and writes the document.
    Index(T),
    /// Take it out of the index.
    Delete,
}

/// One queued write, to the document `entity` / `key` names.
pub(super) struct Job<T> {
    entity: EntityType,
    key: String,
    label: &'static str,
    op: Op<T>,
}

/// One operation per document, in the order each document first arrived:
/// the last one queued for it, whether a write or a delete. The queue keeps
/// the order operations were asked for, so the last says what the document
/// should end as (deleted, or written from the latest state).
pub(super) fn collapse<T>(jobs: Vec<Job<T>>) -> Vec<Job<T>> {
    let mut order: Vec<(EntityType, String)> = Vec::new();
    let mut latest: HashMap<(&'static str, String), Job<T>> = HashMap::new();
    for job in jobs {
        let id = (job.entity.as_str(), job.key.clone());
        if !latest.contains_key(&id) {
            order.push((job.entity, job.key.clone()));
        }
        latest.insert(id, job);
    }
    order
        .into_iter()
        .filter_map(|(entity, key)| latest.remove(&(entity.as_str(), key)))
        .collect()
}

/// The sending end of an index's queue; the thread draining it starts with
/// the first write and ends when the index is dropped.
#[derive(Default)]
pub(crate) struct Queue(Mutex<Option<Sender<Job<IndexFn>>>>);

/// Put `job` on `search_service`'s queue.
fn enqueue(search_service: &Arc<SearchService>, job: Job<IndexFn>) {
    let mut sender = search_service
        .queue
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let job = match sender.as_ref() {
        Some(tx) => match tx.send(job) {
            Ok(()) => return,
            // The consumer is gone (it panicked): start another with this job.
            Err(mpsc::SendError(job)) => job,
        },
        None => job,
    };
    let (tx, rx) = mpsc::channel();
    let weak = Arc::downgrade(search_service);
    let started = std::thread::Builder::new()
        .name("search-index-queue".to_string())
        .spawn(move || consume(weak, rx));
    if let Err(e) = started {
        error!(error = ?e, "Failed to start the search indexing queue");
        return;
    }
    if tx.send(job).is_ok() {
        *sender = Some(tx);
    }
}

/// Drain the queue a batch at a time until the index is dropped.
fn consume(search_service: Weak<SearchService>, rx: Receiver<Job<IndexFn>>) {
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while batch.len() < BATCH_MAX {
            match rx.try_recv() {
                Ok(job) => batch.push(job),
                Err(_) => break,
            }
        }
        let Some(svc) = search_service.upgrade() else {
            return;
        };
        apply_batch(collapse(batch), |job| match job.op {
            Op::Index(write) => write(&svc),
            Op::Delete => svc.delete_by_key(job.entity, &job.key),
        });
        if let Err(e) = svc.commit() {
            error!(error = ?e, "Failed to commit a search indexing batch");
        }
    }
}

/// Apply each job with `apply`, logging a failure or a panic by the
/// document and going on to the next, so one bad job neither stops the
/// batch nor takes the queue's thread down.
fn apply_batch<T>(jobs: Vec<Job<T>>, mut apply: impl FnMut(Job<T>) -> Result<(), BoxError>) {
    for job in jobs {
        let label = job.label;
        let entity = job.entity.as_str();
        let id = job.key.clone();
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| apply(job))) {
            Ok(Ok(())) => debug!(entity, id = %id, "{label} completed"),
            Ok(Err(e)) => error!(entity, id = %id, error = ?e, "Failed to {label}"),
            Err(_) => error!(entity, id = %id, "Search indexing panicked: {label}"),
        }
    }
}

/// Queue a write of the document `entity` / `key`.
fn spawn_indexing_task(
    search_service: Arc<SearchService>,
    label: &'static str,
    entity: EntityType,
    key: String,
    task: impl FnOnce(&SearchService) -> Result<(), BoxError> + Send + 'static,
) {
    enqueue(
        &search_service,
        Job {
            entity,
            key,
            label,
            op: Op::Index(Box::new(task)),
        },
    );
}

/// Queue the removal of the document `entity` / `key`.
fn spawn_delete_task(
    search_service: Arc<SearchService>,
    label: &'static str,
    entity: EntityType,
    key: String,
) {
    enqueue(
        &search_service,
        Job {
            entity,
            key,
            label,
            op: Op::Delete,
        },
    );
}

/// Index a ticket in the background
pub fn spawn_index_ticket(
    search_service: Arc<SearchService>,
    ticket: models::Ticket,
    article_content: Option<models::ArticleContent>,
) {
    spawn_indexing_task(
        search_service,
        "index ticket",
        EntityType::Ticket,
        ticket.id.to_string(),
        move |svc| svc.index_ticket(&ticket, article_content.as_ref()),
    );
}

/// Delete a ticket from the index in the background
pub fn spawn_delete_ticket(search_service: Arc<SearchService>, ticket_id: i32) {
    spawn_delete_task(
        search_service,
        "delete ticket",
        EntityType::Ticket,
        ticket_id.to_string(),
    );
}

/// Index a comment in the background
pub fn spawn_index_comment(
    search_service: Arc<SearchService>,
    comment: models::Comment,
    ticket_title: String,
) {
    spawn_indexing_task(
        search_service,
        "index comment",
        EntityType::Comment,
        comment.id.to_string(),
        move |svc| svc.index_comment(&comment, &ticket_title),
    );
}

/// Delete a comment from the index in the background
pub fn spawn_delete_comment(search_service: Arc<SearchService>, comment_id: i32) {
    spawn_delete_task(
        search_service,
        "delete comment",
        EntityType::Comment,
        comment_id.to_string(),
    );
}

/// Index a documentation page in the background
pub fn spawn_index_documentation(
    search_service: Arc<SearchService>,
    doc_page: models::DocumentationPage,
) {
    spawn_indexing_task(
        search_service,
        "index documentation",
        EntityType::Documentation,
        doc_page.id.to_string(),
        move |svc| svc.index_documentation(&doc_page),
    );
}

/// Delete a documentation page from the index in the background
pub fn spawn_delete_documentation(search_service: Arc<SearchService>, doc_id: i32) {
    spawn_delete_task(
        search_service,
        "delete documentation",
        EntityType::Documentation,
        doc_id.to_string(),
    );
}

/// Index a device in the background
pub fn spawn_index_device(search_service: Arc<SearchService>, device: models::Asset) {
    spawn_indexing_task(
        search_service,
        "index device",
        EntityType::Asset,
        device.id.to_string(),
        move |svc| svc.index_device(&device),
    );
}

/// Re-index a device from current DB state in the background: after a
/// write to one of its records, whose text its document carries.
pub fn spawn_reindex_device(search_service: Arc<SearchService>, device_id: i32) {
    spawn_indexing_task(
        search_service,
        "reindex device",
        EntityType::Asset,
        device_id.to_string(),
        move |svc| svc.reindex_device(device_id),
    );
}

/// Re-index devices by id in the background, one queued job each, as after
/// their kind's schema changes which attributes are staff only.
pub fn spawn_reindex_devices(search_service: Arc<SearchService>, device_ids: Vec<i32>) {
    for id in device_ids {
        spawn_reindex_device(Arc::clone(&search_service), id);
    }
}

/// Delete a device from the index in the background
pub fn spawn_delete_device(search_service: Arc<SearchService>, device_id: i32) {
    spawn_delete_task(
        search_service,
        "delete device",
        EntityType::Asset,
        device_id.to_string(),
    );
}

/// Index a user in the background
pub fn spawn_index_user(
    search_service: Arc<SearchService>,
    user: models::User,
    primary_email: Option<String>,
) {
    spawn_indexing_task(
        search_service,
        "index user",
        EntityType::User,
        user.uuid.to_string(),
        move |svc| svc.index_user(&user, primary_email.as_deref()),
    );
}

/// Re-index a user from current DB state in the background.
///
/// Used after a workspace membership change (add / remove): the user's
/// workspace tags on their search doc are derived from
/// `workspace_members`, so a membership mutation has to refresh the doc.
/// Resolves the user, email, and membership set internally from the
/// uuid, so the caller only needs the uuid.
pub fn spawn_reindex_user(search_service: Arc<SearchService>, user_uuid: uuid::Uuid) {
    spawn_indexing_task(
        search_service,
        "reindex user",
        EntityType::User,
        user_uuid.to_string(),
        move |svc| svc.reindex_user(user_uuid),
    );
}

/// Delete a user from the index in the background
pub fn spawn_delete_user(search_service: Arc<SearchService>, user_uuid: String) {
    spawn_delete_task(search_service, "delete user", EntityType::User, user_uuid);
}

/// Index a project in the background
pub fn spawn_index_project(search_service: Arc<SearchService>, project: models::Project) {
    spawn_indexing_task(
        search_service,
        "index project",
        EntityType::Project,
        project.id.to_string(),
        move |svc| svc.index_project(&project),
    );
}

/// Delete a project from the index in the background
pub fn spawn_delete_project(search_service: Arc<SearchService>, project_id: i32) {
    spawn_delete_task(
        search_service,
        "delete project",
        EntityType::Project,
        project_id.to_string(),
    );
}

/// A requested workspace reindex.
pub enum WorkspaceReindex {
    /// A pass started on a blocking thread; the handle resolves when it (and
    /// any pass queued behind it) has committed.
    Started(
        tokio::task::JoinHandle<
            Result<
                super::workspace_reindex::WorkspaceIndexStats,
                Box<dyn std::error::Error + Send + Sync>,
            >,
        >,
    ),
    /// A reindex of the workspace was already pending or running, and covers
    /// this request.
    Joined,
}

/// Rebuild one workspace's search documents in the background
/// (`SearchService::reindex_workspace`). Other workspaces' documents are left
/// alone. Call after committing rows written outside the incremental index
/// hooks, such as a workspace import.
pub fn spawn_reindex_workspace(
    search_service: Arc<SearchService>,
    workspace_id: i32,
) -> WorkspaceReindex {
    use super::workspace_reindex::Admission;
    if search_service.admit_workspace_reindex(workspace_id) == Admission::Joined {
        return WorkspaceReindex::Joined;
    }
    WorkspaceReindex::Started(tokio::task::spawn_blocking(move || {
        let result = search_service.run_admitted_workspace_reindex(workspace_id);
        if let Err(e) = &result {
            error!(workspace_id, error = ?e, "Failed to reindex a workspace");
        }
        result
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(entity: EntityType, key: &str, op: Op<&'static str>) -> Job<&'static str> {
        Job {
            entity,
            key: key.to_string(),
            label: "test",
            op,
        }
    }

    fn ops(jobs: Vec<Job<&'static str>>) -> Vec<(String, &'static str)> {
        collapse(jobs)
            .into_iter()
            .map(|j| {
                let what = match j.op {
                    Op::Index(write) => write,
                    Op::Delete => "delete",
                };
                (format!("{}-{}", j.entity.as_str(), j.key), what)
            })
            .collect()
    }

    #[test]
    fn repeats_collapse_to_the_newest_write_in_arrival_order() {
        let jobs = vec![
            job(EntityType::Asset, "7", Op::Index("first")),
            job(EntityType::Ticket, "7", Op::Index("ticket")),
            job(EntityType::Asset, "7", Op::Index("second")),
            job(EntityType::Asset, "8", Op::Index("other")),
            job(EntityType::Asset, "7", Op::Index("third")),
        ];
        assert_eq!(
            ops(jobs),
            vec![
                ("device-7".to_string(), "third"),
                ("ticket-7".to_string(), "ticket"),
                ("device-8".to_string(), "other"),
            ]
        );
    }

    #[test]
    fn the_last_operation_queued_for_a_document_wins() {
        let jobs = vec![
            // Archived, then restored: the page goes back in.
            job(EntityType::Documentation, "3", Op::Delete),
            job(EntityType::Documentation, "3", Op::Index("restored")),
            // Written, then deleted: it stays out.
            job(EntityType::Asset, "7", Op::Index("before")),
            job(EntityType::Asset, "7", Op::Delete),
            job(EntityType::Asset, "9", Op::Delete),
            job(EntityType::Asset, "9", Op::Delete),
        ];
        assert_eq!(
            ops(jobs),
            vec![
                ("documentation-3".to_string(), "restored"),
                ("device-7".to_string(), "delete"),
                ("device-9".to_string(), "delete"),
            ]
        );
    }

    #[test]
    fn a_panicking_job_is_logged_and_the_batch_goes_on() {
        let jobs = vec![
            job(EntityType::Asset, "1", Op::Index("panics")),
            job(EntityType::Asset, "2", Op::Index("writes")),
            job(EntityType::Asset, "3", Op::Delete),
        ];
        let mut done = Vec::new();
        apply_batch(jobs, |j| {
            if let Op::Index("panics") = j.op {
                panic!("a job that panics");
            }
            done.push(j.key);
            Ok(())
        });
        assert_eq!(done, vec!["2".to_string(), "3".to_string()]);
    }
}
