//! Workspace imports as jobs: an export uploaded into an existing workspace,
//! checked, confirmed, followed and, when unwanted, reset.
//!
//! A job moves through these states (`workspace_import_jobs.state`):
//!
//! ```text
//! uploaded --check--> checking --+--> previewed --confirm--> importing --+--> imported --reset--> reset
//!    ^                           |        |                              |
//!    +------- refused -----------+        +-- check again                +--> failed --retry files--> importing
//!                                                                             failed --reset--> reset
//! uploaded, previewed --reset (discard)--> reset
//! ```
//!
//! - **Upload** parks the archive at `workspace-imports/{job}.nosdesk` in the
//!   workspace's storage, outside `imports/`, whose orphan sweep deletes what
//!   the CSV importer doesn't know. Nothing reads it yet.
//! - **Check** (background) reads the archive (the version gate), checks the
//!   workspace is untouched and plans the import, in a read-only transaction:
//!   the preview is the plan's report. A refusal puts the job back to
//!   `uploaded` with the refusal, to be checked again.
//! - **Confirm** (background) imports the rows in one transaction
//!   (`workspace_archive::apply::import_into`) that also records on the job
//!   that they committed and the accounts they created; reindexes the
//!   workspace for search; then restores the files. Files that fail leave the
//!   job `failed` with `files_partial` and the archive kept for **retry
//!   files**. A refusal (the workspace changed since the check) puts the job
//!   back to `uploaded`.
//! - **Reset** of an imported (or partly imported) job takes the workspace
//!   back to how it was provisioned (`apply::reset`) and removes the accounts
//!   the import created, in one transaction. Of a job that imported nothing,
//!   it discards the job.
//!
//! One live job per workspace (the table's partial unique index); one
//! background check or import per process, since each holds its archive in
//! memory ([`RUNNING`]). A running job marks itself alive every
//! [`HEARTBEAT`] without audit rows (its update trigger skips a change of
//! `updated_at`, `stage` and `files_restored` alone). A job whose process
//! died is put back by [`settle`], on the next read of the job or by the
//! daily prune: a check goes back to `uploaded`; an import goes back to
//! `previewed` when its rows never committed, else to `failed` with
//! `files_partial`. Jobs keep their report for [`KEEP_FOR`]; then the prune
//! drops the archive and the people in the report and keeps the counts.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use diesel::prelude::*;
use serde_json::{json, Value};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::middleware::workspace_context::DeploymentMode;
use crate::models::{WorkspaceImportJob, WorkspaceImportJobChange};
use crate::repository::workspace_import_jobs as repo;
use crate::services::backup::BackupError;
use crate::services::imports::refusal::Refused;
use crate::services::search::{indexing_tasks, SearchService};
use crate::services::uploaded_archive::spool;
use crate::services::workspace_archive::apply::{self, ApplyError};
use crate::services::workspace_archive::files::{self, FilePlan};
use crate::services::workspace_archive::plan::{self, ImportTarget, PlanOptions, Runner};
use crate::services::workspace_archive::refs::{Entity, Lookup};
use crate::services::workspace_import::{read_archive, ArchiveContents};
use crate::sync::actor::ActorContext;
use crate::sync::session::{with_actor_bypass_context, with_actor_context};
use crate::utils::storage::{Storage, WorkspaceScopedStorage};

pub const UPLOADED: &str = "uploaded";
pub const CHECKING: &str = "checking";
pub const PREVIEWED: &str = "previewed";
pub const IMPORTING: &str = "importing";
pub const IMPORTED: &str = "imported";
pub const FAILED: &str = "failed";
pub const RESET: &str = "reset";

/// Where waiting archives are parked in a workspace's storage. Not under
/// `imports/` (the CSV importer's orphan sweep) and left out of exports.
pub const ARCHIVE_PREFIX: &str = "workspace-imports/";

/// How often a running job marks itself alive.
pub const HEARTBEAT: Duration = Duration::from_secs(30);

/// How long a running job may go without a heartbeat before it is put back.
pub const STALLED_AFTER: Duration = Duration::from_secs(5 * 60);

/// How long a job keeps its archive and the people in its report, and how
/// long a reset stays available (N4: as CSV imports).
pub const KEEP_FOR: chrono::Duration = chrono::Duration::days(30);

/// The upload cap when `NOSDESK_MAX_IMPORT_BYTES` isn't set: 1 GiB, the
/// export's own cap. An instance short of memory lowers it (design P-D8:
/// 512 MiB).
pub const DEFAULT_MAX_UPLOAD_BYTES: u64 = 1024 * 1024 * 1024;

/// One background check or import per process: each holds its archive in
/// memory, about three times its size while it unseals and parses.
static RUNNING: Semaphore = Semaphore::const_new(1);

/// The upload cap (`NOSDESK_MAX_IMPORT_BYTES`, else 1 GiB).
pub fn max_upload_bytes() -> u64 {
    std::env::var("NOSDESK_MAX_IMPORT_BYTES")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MAX_UPLOAD_BYTES)
}

/// The storage key (workspace-relative) of job `id`'s archive.
pub fn archive_key(id: Uuid) -> String {
    format!("{ARCHIVE_PREFIX}{id}.nosdesk")
}

/// Whether the first bytes of an upload are a sealed archive's.
pub fn is_sealed(head: &[u8]) -> bool {
    head.starts_with(b"NODB")
}

// ---------------------------------------------------------------------------
// Who runs it
// ---------------------------------------------------------------------------

/// Who starts and drives a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// The workspace owner, in the app (self-hosted only).
    Owner { user: Uuid },
    /// A platform admin.
    Operator { user: Option<Uuid> },
}

impl Origin {
    /// The `runner` column.
    pub fn runner(&self) -> &'static str {
        match self {
            Origin::Owner { .. } => "owner",
            Origin::Operator { .. } => "operator",
        }
    }

    pub fn user(&self) -> Option<Uuid> {
        match self {
            Origin::Owner { user } => Some(*user),
            Origin::Operator { user } => *user,
        }
    }

    /// The import target for workspace `workspace_id`. An owner's import is
    /// self-hosted (the tenant route isn't served on hosted); an operator's
    /// follows the deployment.
    pub fn target(&self, workspace_id: i32) -> ImportTarget {
        match self {
            Origin::Owner { user } => ImportTarget {
                workspace_id,
                mode: DeploymentMode::SelfHosted,
                runner: Runner::SelfHostedOwner { user: *user },
            },
            Origin::Operator { user } => ImportTarget {
                workspace_id,
                mode: DeploymentMode::current(),
                runner: Runner::Operator { user: *user },
            },
        }
    }

    fn actor(&self, workspace_id: i32) -> ActorContext {
        match self.user() {
            Some(user) => ActorContext::user(user, None).with_workspace(workspace_id),
            None => ActorContext::system("workspace_import").with_workspace(workspace_id),
        }
    }
}

/// What a running job needs from the server.
#[derive(Clone)]
pub struct Runtime {
    pub pool: Pool,
    /// The instance storage (unscoped); jobs scope it to their workspace.
    pub storage: Arc<dyn Storage>,
    pub search: Option<Arc<SearchService>>,
}

impl Runtime {
    fn scoped(&self, workspace_id: i32) -> Arc<dyn Storage> {
        WorkspaceScopedStorage::arc(self.storage.clone(), workspace_id)
    }
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// A refusal as a job stores it and the routes answer with it.
pub fn refusal_value(refused: &Refused) -> Value {
    json!({ "code": refused.code, "message": refused.message, "args": refused.args })
}

/// An apply or reset refusal as the shared refusal: the codes #859 returns.
pub fn apply_refused(e: &ApplyError) -> Refused {
    let refused = Refused::new(e.code(), e.to_string());
    match e {
        ApplyError::NotUntouched { found } => refused.arg("found", json!(found)),
        ApplyError::NoWorkspace { workspace_id } | ApplyError::Busy { workspace_id } => {
            refused.arg("workspace_id", *workspace_id)
        }
        ApplyError::Refused(r) => {
            let inner = r.to_refused();
            let mut out = Refused::new(inner.code, inner.message);
            out.args = inner.args;
            out
        }
        ApplyError::NotElevated | ApplyError::Failed(_) => {
            Refused::new("import_failed", "The import failed. Nothing was imported.")
        }
    }
}

/// Why an archive couldn't be read, as the shared refusal.
fn read_refused(e: BackupError) -> Refused {
    match e {
        BackupError::NotImportable(refusal) => refusal.to_refused(),
        BackupError::InvalidPassword | BackupError::EncryptionError(_) => Refused::new(
            "workspace_import.password_wrong",
            "The password doesn't open this export.",
        ),
        e => {
            tracing::warn!(error = %e, "workspace import: unreadable archive");
            Refused::new(
                "workspace_import.unreadable",
                "This file isn't a Nosdesk workspace export, or it is damaged.",
            )
        }
    }
}

pub fn password_required() -> Refused {
    Refused::new(
        "workspace_import.password_required",
        "This export is sealed with a password. Enter it to continue.",
    )
}

// ---------------------------------------------------------------------------
// Database helpers
// ---------------------------------------------------------------------------

/// Run `f` as `actor` pinned to the job's workspace (row security applies),
/// off the async worker.
pub async fn in_workspace<T, F>(pool: &Pool, actor: ActorContext, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&mut DbConnection) -> QueryResult<T> + Send + 'static,
{
    let pool = pool.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        with_actor_context(&mut conn, &actor, f).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Record `change` on the job if it is still in one of `from`.
async fn record(
    pool: &Pool,
    actor: ActorContext,
    workspace_id: i32,
    id: Uuid,
    from: &'static [&'static str],
    change: WorkspaceImportJobChange,
) -> Option<WorkspaceImportJob> {
    match in_workspace(pool, actor, move |conn| {
        repo::transition(conn, workspace_id, id, from, change)
    })
    .await
    {
        Ok(job) => job,
        Err(e) => {
            tracing::error!(id = %id, error = %e, "workspace import: recording the job failed");
            None
        }
    }
}

/// Marks a running job alive every [`HEARTBEAT`] until dropped.
struct Heartbeat {
    task: tokio::task::JoinHandle<()>,
}

impl Heartbeat {
    fn start(pool: Pool, workspace_id: i32, id: Uuid, state: &'static str) -> Self {
        let task = tokio::spawn(async move {
            let mut every = tokio::time::interval(HEARTBEAT);
            every.tick().await;
            loop {
                every.tick().await;
                let actor =
                    ActorContext::system("workspace_import:heartbeat").with_workspace(workspace_id);
                match in_workspace(&pool, actor, move |conn| {
                    repo::heartbeat(conn, id, state, None, None)
                })
                .await
                {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(id = %id, error = %e, "workspace import heartbeat failed")
                    }
                }
            }
        });
        Self { task }
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Record where a running job is.
async fn set_stage(pool: &Pool, workspace_id: i32, id: Uuid, stage: &'static str) {
    let actor = ActorContext::system("workspace_import:stage").with_workspace(workspace_id);
    if let Err(e) = in_workspace(pool, actor, move |conn| {
        repo::heartbeat(conn, id, IMPORTING, Some(stage), None)
    })
    .await
    {
        tracing::warn!(id = %id, error = %e, "workspace import: recording the stage failed");
    }
}

/// Read job `id`'s stored archive.
async fn read_stored(
    rt: &Runtime,
    workspace_id: i32,
    id: Uuid,
    password: Option<String>,
) -> Result<ArchiveContents, Refused> {
    let storage = rt.scoped(workspace_id);
    let spooled = spool::fetch(storage.as_ref(), &archive_key(id), None)
        .await
        .map_err(|e| {
            tracing::error!(id = %id, error = ?e, "workspace import: archive unavailable");
            Refused::new(
                "workspace_import.archive_missing",
                "The uploaded export is no longer stored. Upload it again.",
            )
        })?;
    tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(spooled.path()).map_err(|e| {
            tracing::error!(error = %e, "workspace import: reading the archive failed");
            Refused::new(
                "workspace_import.unreadable",
                "The export couldn't be read.",
            )
        })?;
        drop(spooled);
        read_archive(&bytes, password.as_deref()).map_err(read_refused)
    })
    .await
    .map_err(|e| Refused::new("workspace_import.unreadable", e.to_string()))?
}

/// The manifest facts the preview and report show.
fn source_of(contents: &ArchiveContents) -> Value {
    let m = &contents.manifest;
    json!({
        "nosdesk_version": m.nosdesk_version,
        "format": m.workspace_export_format_version,
        "created_at": m.created_at,
        "mode": m.source_mode,
        "workspace_slug": m.workspace_slug,
        "origin": m.source_origin,
        "custom_domain": m.custom_domain,
        "sealed": m.sealed,
        "secrets": m.secrets,
        "files": contents.files.len(),
    })
}

/// The accounts a plan's report created.
fn created_by(report: &plan::ImportReport) -> Vec<Uuid> {
    report
        .people
        .iter()
        .filter(|p| p.is_new())
        .map(|p| p.target_uuid)
        .collect()
}

// ---------------------------------------------------------------------------
// Check
// ---------------------------------------------------------------------------

/// What a check came to: the preview, or a refusal (with the plan's report
/// when the plan was what refused).
struct Checked {
    report: Value,
    refused: Option<Refused>,
}

/// Run a check of job `id` in the background. The caller has put it in
/// `checking`.
pub fn spawn_check(
    rt: Runtime,
    origin: Origin,
    workspace_id: i32,
    id: Uuid,
    password: Option<String>,
    regenerate_uuids: bool,
) {
    tokio::spawn(async move {
        let _beat = Heartbeat::start(rt.pool.clone(), workspace_id, id, CHECKING);
        let _permit = RUNNING.acquire().await;
        let checked = check(&rt, &origin, workspace_id, id, password, regenerate_uuids).await;
        let now = Utc::now();
        let change = match checked {
            Ok(Checked {
                report,
                refused: None,
            }) => WorkspaceImportJobChange {
                state: Some(PREVIEWED.into()),
                report: Some(Some(report)),
                error: Some(None),
                checked_at: Some(Some(now)),
                ..Default::default()
            },
            Ok(Checked {
                report,
                refused: Some(refused),
            }) => WorkspaceImportJobChange {
                state: Some(UPLOADED.into()),
                report: Some(Some(report)),
                error: Some(Some(refusal_value(&refused))),
                checked_at: Some(Some(now)),
                ..Default::default()
            },
            Err(refused) => WorkspaceImportJobChange {
                state: Some(UPLOADED.into()),
                report: Some(None),
                error: Some(Some(refusal_value(&refused))),
                checked_at: Some(Some(now)),
                ..Default::default()
            },
        };
        record(
            &rt.pool,
            origin.actor(workspace_id),
            workspace_id,
            id,
            &[CHECKING],
            change,
        )
        .await;
    });
}

/// Read the archive, check the workspace is untouched and plan the import,
/// writing nothing.
async fn check(
    rt: &Runtime,
    origin: &Origin,
    workspace_id: i32,
    id: Uuid,
    password: Option<String>,
    regenerate_uuids: bool,
) -> Result<Checked, Refused> {
    let contents = Arc::new(read_stored(rt, workspace_id, id, password).await?);
    let held = apply::held_files(rt.storage.clone(), workspace_id)
        .await
        .map_err(|e| {
            tracing::error!(id = %id, error = ?e, "workspace import: listing files failed");
            Refused::new("import_failed", "The workspace's files couldn't be listed.")
        })?;
    let source = source_of(&contents);
    let target = origin.target(workspace_id);
    let actor = origin.actor(workspace_id);
    let pool = rt.pool.clone();
    let planned = tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        // elevated-in-workspace: the untouched check and the plan read this workspace across every table, and uniqueness and email holders instance-wide, in a read-only transaction.
        with_actor_bypass_context(&mut conn, &actor, |conn| {
            diesel::sql_query("SET LOCAL transaction_read_only = on").execute(conn)?;
            let found = apply::untouched(conn, workspace_id, &held)
                .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?;
            if !found.is_empty() {
                return Ok(Err(found));
            }
            let plan = plan::plan_import(
                conn,
                &contents,
                &target,
                PlanOptions {
                    regenerate_row_uuids: regenerate_uuids,
                },
            )
            .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))?;
            Ok(Ok(plan.report))
        })
        .map_err(|e: diesel::result::Error| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    match planned {
        Err(e) => {
            tracing::error!(id = %id, error = %e, "workspace import: check failed");
            Err(Refused::new(
                "import_failed",
                "The export couldn't be checked against this workspace.",
            ))
        }
        Ok(Err(found)) => Err(apply_refused(&ApplyError::NotUntouched { found })),
        Ok(Ok(report)) => {
            let refused = (!report.refusals.is_empty()).then(|| {
                apply_refused(&ApplyError::Refused(
                    crate::services::workspace_import::ImportRefusal::Plan(report.refusals.clone()),
                ))
            });
            Ok(Checked {
                report: json!({ "source": source, "plan": report }),
                refused,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Import and retry files
// ---------------------------------------------------------------------------

/// Run the import of job `id` in the background: rows, search, files. With
/// `files_only`, only the files (retry files). The caller has put it in
/// `importing`.
pub fn spawn_import(
    rt: Runtime,
    origin: Origin,
    workspace_id: i32,
    id: Uuid,
    password: Option<String>,
    files_only: bool,
) {
    tokio::spawn(async move {
        let _beat = Heartbeat::start(rt.pool.clone(), workspace_id, id, IMPORTING);
        let _permit = RUNNING.acquire().await;
        import(&rt, &origin, workspace_id, id, password, files_only).await;
    });
}

async fn import(
    rt: &Runtime,
    origin: &Origin,
    workspace_id: i32,
    id: Uuid,
    password: Option<String>,
    files_only: bool,
) {
    let actor = origin.actor(workspace_id);
    let contents = match read_stored(rt, workspace_id, id, password).await {
        Ok(contents) => Arc::new(contents),
        Err(refused) => {
            // Nothing new was written: the job goes back to where it was.
            let back = if files_only { FAILED } else { PREVIEWED };
            let change = WorkspaceImportJobChange {
                state: Some(back.into()),
                stage: Some(None),
                error: Some(Some(refusal_value(&refused))),
                ..Default::default()
            };
            record(&rt.pool, actor, workspace_id, id, &[IMPORTING], change).await;
            return;
        }
    };

    let mut report = match current_report(rt, workspace_id, id).await {
        Some(report) => report,
        None => json!({}),
    };
    let plan = if files_only {
        // The rows committed in an earlier run: the file plan comes back from
        // the keys it recorded.
        FilePlan::build(&contents, &StoredKeys::from_report(&report))
    } else {
        set_stage(&rt.pool, workspace_id, id, "rows").await;
        let plan = match import_rows(rt, origin, workspace_id, id, contents.clone()).await {
            Ok((applied, plan)) => {
                report = applied;
                plan
            }
            Err((state, refused)) => {
                record(
                    &rt.pool,
                    actor,
                    workspace_id,
                    id,
                    &[IMPORTING],
                    WorkspaceImportJobChange {
                        state: Some(state.into()),
                        stage: Some(None),
                        error: Some(Some(refusal_value(&refused))),
                        finished_at: Some((state == FAILED).then(Utc::now)),
                        ..Default::default()
                    },
                )
                .await;
                return;
            }
        };
        if let Some(search) = &rt.search {
            indexing_tasks::spawn_reindex_workspace(Arc::clone(search), workspace_id);
        }
        plan
    };

    set_stage(&rt.pool, workspace_id, id, "files").await;
    let files = files::restore(rt.storage.clone(), workspace_id, &contents, &plan).await;
    drop(contents);
    let restored = i32::try_from(files.restored).unwrap_or(i32::MAX);
    if let Some(obj) = report.as_object_mut() {
        obj.insert("files".into(), json!(files));
    }
    if files.failed.is_some() {
        let refused = Refused::new(
            "files_partial",
            format!(
                "The rows were imported, but restoring files stopped after {} of them. Retry the \
                 files.",
                files.restored
            ),
        )
        .arg("restored", files.restored)
        .arg("present", files.present)
        .arg("skipped", files.skipped.len());
        record(
            &rt.pool,
            actor,
            workspace_id,
            id,
            &[IMPORTING],
            WorkspaceImportJobChange {
                state: Some(FAILED.into()),
                stage: Some(None),
                report: Some(Some(report)),
                error: Some(Some(refusal_value(&refused))),
                files_restored: Some(restored),
                finished_at: Some(Some(Utc::now())),
                ..Default::default()
            },
        )
        .await;
        return;
    }
    if let Some(obj) = report.as_object_mut() {
        obj.remove(FILE_KEYS);
    }
    let done = record(
        &rt.pool,
        actor,
        workspace_id,
        id,
        &[IMPORTING],
        WorkspaceImportJobChange {
            state: Some(IMPORTED.into()),
            stage: Some(None),
            archive_key: Some(None),
            report: Some(Some(report)),
            error: Some(None),
            files_restored: Some(restored),
            finished_at: Some(Some(Utc::now())),
            ..Default::default()
        },
    )
    .await;
    if done.is_some() {
        delete_archive(rt.scoped(workspace_id).as_ref(), id).await;
    }
}

async fn current_report(rt: &Runtime, workspace_id: i32, id: Uuid) -> Option<Value> {
    let actor = ActorContext::system("workspace_import:read").with_workspace(workspace_id);
    in_workspace(&rt.pool, actor, move |conn| {
        repo::get(conn, workspace_id, id)
    })
    .await
    .ok()
    .flatten()
    .and_then(|job| job.report)
}

/// Import the rows, recording on the job, in the same transaction, that they
/// committed, whom they created and where the archive's files land. Returns
/// the report and the file plan; on a refusal, the state the job goes back
/// to and why.
async fn import_rows(
    rt: &Runtime,
    origin: &Origin,
    workspace_id: i32,
    id: Uuid,
    contents: Arc<ArchiveContents>,
) -> Result<(Value, FilePlan), (&'static str, Refused)> {
    let held = apply::held_files(rt.storage.clone(), workspace_id)
        .await
        .map_err(|e| {
            tracing::error!(id = %id, error = ?e, "workspace import: listing files failed");
            (
                PREVIEWED,
                Refused::new("import_failed", "The workspace's files couldn't be listed."),
            )
        })?;
    let target = origin.target(workspace_id);
    let actor = origin.actor(workspace_id);
    let pool = rt.pool.clone();
    let source = source_of(&contents);
    let outcome = tokio::task::spawn_blocking(move || -> Result<(Value, FilePlan), ApplyError> {
        let mut conn = pool.get().map_err(|e| {
            ApplyError::Failed(BackupError::DatabaseError(
                diesel::result::Error::QueryBuilderError(e.to_string().into()),
            ))
        })?;
        // elevated-in-workspace: the import loads this workspace's rows on the owner role, which updates its workspaces row and clears its seeded configuration.
        with_actor_bypass_context(&mut conn, &actor, |conn| {
            let job = repo::get_for_update(conn, workspace_id, id)?;
            let regenerate = job.as_ref().is_some_and(|j| j.regenerate_uuids);
            if job.is_none_or(|j| j.state != IMPORTING) {
                return Err(ApplyError::Busy { workspace_id });
            }
            let applied = apply::import_into(
                conn,
                &contents,
                &target,
                PlanOptions {
                    regenerate_row_uuids: regenerate,
                },
                &held,
            )?;
            let report = json!({
                "source": source,
                "plan": applied.report,
                "cleared": applied.cleared,
                "tables_imported": applied.tables_imported,
                "rows_imported": applied.rows_imported,
                "data_epoch": applied.fence.epoch,
                FILE_KEYS: file_keys(&contents, &applied.files),
            });
            // The job's own change is audited like any other.
            diesel::sql_query("SET LOCAL nosdesk.in_audit_read = 'false'").execute(conn)?;
            repo::transition(
                conn,
                workspace_id,
                id,
                &[IMPORTING],
                WorkspaceImportJobChange {
                    applied_at: Some(Some(Utc::now())),
                    created_people: Some(created_by(&applied.report)),
                    report: Some(Some(report.clone())),
                    error: Some(None),
                    ..Default::default()
                },
            )?;
            Ok((report, applied.files))
        })
    })
    .await;
    match outcome {
        Ok(Ok(done)) => Ok(done),
        Ok(Err(e)) => {
            let refused = apply_refused(&e);
            match e {
                ApplyError::NotUntouched { .. }
                | ApplyError::Refused(_)
                | ApplyError::NoWorkspace { .. }
                | ApplyError::Busy { .. } => Err((UPLOADED, refused)),
                ApplyError::NotElevated | ApplyError::Failed(_) => {
                    tracing::error!(id = %id, error = %e, "workspace import failed");
                    Err((FAILED, refused))
                }
            }
        }
        Err(e) => {
            tracing::error!(id = %id, error = %e, "workspace import task failed");
            Err((
                FAILED,
                Refused::new("import_failed", "The import failed. Nothing was imported."),
            ))
        }
    }
}

/// The report key holding where each archive file lands, kept until the
/// files are restored so retry files can rebuild the plan.
const FILE_KEYS: &str = "file_keys";

/// `{archive path: storage key}` for every file the plan restores.
fn file_keys(contents: &ArchiveContents, plan: &FilePlan) -> Value {
    let mut keys = serde_json::Map::new();
    for (name, _) in &contents.files {
        let path = name.strip_prefix("files/").unwrap_or(name);
        if let Some(key) = plan.key_of(path) {
            keys.insert(path.to_string(), Value::String(key.to_string()));
        }
    }
    Value::Object(keys)
}

/// The ticket and asset keys a committed import gave the files it
/// restores, read back from [`file_keys`]: notes images and asset media
/// carry their owner's id as the path's second segment.
struct StoredKeys(std::collections::HashMap<(Entity, String), String>);

impl StoredKeys {
    fn from_report(report: &Value) -> Self {
        let mut keys = std::collections::HashMap::new();
        if let Some(stored) = report.get(FILE_KEYS).and_then(Value::as_object) {
            for (path, key) in stored {
                let (Some(key), mut old) = (key.as_str(), path.split('/')) else {
                    continue;
                };
                let mut new = key.split('/');
                let entity = match (old.next(), new.next()) {
                    (Some("tickets"), Some("tickets")) => Entity::Ticket,
                    (Some("assets"), Some("assets")) => Entity::Asset,
                    _ => continue,
                };
                if let (Some(from), Some(to)) = (old.next(), new.next()) {
                    keys.insert((entity, from.to_string()), to.to_string());
                }
            }
        }
        Self(keys)
    }
}

impl Lookup for StoredKeys {
    fn user(&self, uuid: &str) -> Option<String> {
        Some(uuid.to_string())
    }

    fn key(&self, entity: Entity, key: &str) -> Option<String> {
        self.0.get(&(entity, key.to_string())).cloned()
    }
}

/// Delete job `id`'s stored archive. A failure is only storage; the prune
/// tries again.
pub async fn delete_archive(storage: &dyn Storage, id: Uuid) {
    match storage.delete_file(&archive_key(id)).await {
        Ok(_) | Err(crate::utils::storage::StorageError::NotFound(_)) => {}
        Err(e) => {
            tracing::warn!(id = %id, error = ?e, "workspace import: deleting the archive failed")
        }
    }
}

// ---------------------------------------------------------------------------
// Reset
// ---------------------------------------------------------------------------

/// Why a reset didn't happen.
#[derive(Debug)]
pub enum ResetError {
    NotFound,
    /// The job can't be reset as it stands; `status` is the HTTP status.
    Refused {
        status: u16,
        refused: Refused,
    },
    Failed(String),
}

/// Reset job `id`. An import that committed rows takes the workspace back
/// to how it was provisioned (`apply::reset`, seeding the welcome page for
/// `seed_author`) and removes the accounts it created, in one transaction,
/// then deletes the workspace's files. A job that imported nothing is
/// discarded. Either way the job ends `reset` and its archive is deleted.
pub async fn reset(
    rt: &Runtime,
    origin: &Origin,
    workspace_id: i32,
    id: Uuid,
    mode: DeploymentMode,
    seed_author: Option<Uuid>,
) -> Result<WorkspaceImportJob, ResetError> {
    let actor = origin.actor(workspace_id);
    let by = origin.user();
    let pool = rt.pool.clone();
    let outcome =
        tokio::task::spawn_blocking(move || -> Result<(WorkspaceImportJob, bool), ResetError> {
            let mut conn = pool.get().map_err(|e| ResetError::Failed(e.to_string()))?;
            let mut refusal: Option<ResetError> = None;
            // elevated-in-workspace: a reset deletes this workspace's content on the owner role (asset_disposals takes deletes only there) and the accounts its import created.
            let result = with_actor_bypass_context(&mut conn, &actor, |conn| {
                let Some(job) = repo::get_for_update(conn, workspace_id, id)? else {
                    refusal = Some(ResetError::NotFound);
                    return Err(diesel::result::Error::RollbackTransaction);
                };
                if let Some(refused) = reset_refusal(&job) {
                    refusal = Some(refused);
                    return Err(diesel::result::Error::RollbackTransaction);
                }
                let applied = job.applied_at.is_some();
                let mut report = job.report.clone().unwrap_or_else(|| json!({}));
                if applied {
                    let done = match apply::reset(conn, workspace_id, mode, seed_author) {
                        Ok(done) => done,
                        Err(e) => {
                            let status = match e {
                                ApplyError::Busy { .. } => 409,
                                ApplyError::NoWorkspace { .. } => 404,
                                _ => 500,
                            };
                            refusal = Some(if status == 500 {
                                ResetError::Failed(e.to_string())
                            } else {
                                ResetError::Refused {
                                    status,
                                    refused: apply_refused(&e),
                                }
                            });
                            return Err(diesel::result::Error::RollbackTransaction);
                        }
                    };
                    let people: Vec<Uuid> = job
                        .created()
                        .into_iter()
                        .filter(|p| Some(*p) != job.requested_by && Some(*p) != by)
                        .collect();
                    let (purged, kept) = repo::remove_created_people(conn, workspace_id, &people)?;
                    if let Some(obj) = report.as_object_mut() {
                        obj.insert(
                            "reset".into(),
                            json!({
                                "cleared": done.cleared,
                                "people_removed": purged,
                                "people_kept": kept,
                                "data_epoch": done.fence.epoch,
                            }),
                        );
                    }
                }
                // The job's own change is audited, as the person who reset it
                // (the reset's seeding ran as a system actor, on the app role).
                crate::sync::session::set_actor(conn, &actor)?;
                diesel::sql_query("SET LOCAL nosdesk.in_audit_read = 'false'").execute(conn)?;
                let now = Utc::now();
                let job = repo::transition(
                    conn,
                    workspace_id,
                    id,
                    &[UPLOADED, PREVIEWED, IMPORTED, FAILED],
                    WorkspaceImportJobChange {
                        state: Some(RESET.into()),
                        stage: Some(None),
                        archive_key: Some(None),
                        report: Some(Some(report)),
                        reset_by: Some(by),
                        reset_at: Some(Some(now)),
                        ..Default::default()
                    },
                )?
                .ok_or(diesel::result::Error::NotFound)?;
                Ok((job, applied))
            });
            match (result, refusal) {
                (Ok(done), _) => Ok(done),
                (Err(_), Some(refusal)) => Err(refusal),
                (Err(e), None) => Err(ResetError::Failed(e.to_string())),
            }
        })
        .await
        .map_err(|e| ResetError::Failed(e.to_string()))??;

    let (job, applied) = outcome;
    let scoped = rt.scoped(workspace_id);
    delete_archive(scoped.as_ref(), id).await;
    if applied {
        if let Err(e) = apply::clear_files(rt.storage.clone(), workspace_id).await {
            tracing::error!(id = %id, error = ?e, "workspace import reset: deleting files failed");
        }
        if let Some(search) = &rt.search {
            indexing_tasks::spawn_reindex_workspace(Arc::clone(search), workspace_id);
        }
    }
    Ok(job)
}

/// Why job `job` can't be reset now, if it can't.
fn reset_refusal(job: &WorkspaceImportJob) -> Option<ResetError> {
    match job.state.as_str() {
        CHECKING | IMPORTING => Some(ResetError::Refused {
            status: 409,
            refused: Refused::new(
                "workspace_import.running",
                "This import is running. Reset it once it finishes.",
            ),
        }),
        RESET => Some(ResetError::Refused {
            status: 409,
            refused: Refused::new(
                "workspace_import.already_reset",
                "This import was already reset.",
            ),
        }),
        _ if job.applied_at.is_some() && job.pruned_at.is_some() => Some(ResetError::Refused {
            status: 410,
            refused: Refused::new(
                "workspace_import.reset_window_closed",
                "An import can be reset for 30 days. That window has closed.",
            )
            .arg("days", KEEP_FOR.num_days()),
        }),
        _ => None,
    }
}

/// What a job offers now, for the client's buttons.
pub fn actions(job: &WorkspaceImportJob) -> Value {
    // A failed job whose rows committed failed at its files.
    let files_partial =
        job.state == FAILED && job.applied_at.is_some() && job.archive_key.is_some();
    json!({
        "check": matches!(job.state.as_str(), UPLOADED | PREVIEWED) && job.archive_key.is_some(),
        "confirm": job.state == PREVIEWED,
        "retry_files": files_partial,
        "reset": reset_refusal(job).is_none(),
        "invite": job.applied_at.is_some()
            && job.pruned_at.is_none()
            && !matches!(job.state.as_str(), RESET | IMPORTING),
    })
}

/// The job as the routes answer with it.
pub fn view(job: &WorkspaceImportJob) -> Value {
    json!({
        "id": job.id,
        "workspace_id": job.workspace_id,
        "runner": job.runner,
        "state": job.state,
        "stage": job.stage,
        "archive": {
            "name": job.archive_name,
            "size": job.archive_size,
            "sealed": job.sealed,
            "stored": job.archive_key.is_some(),
        },
        "regenerate_uuids": job.regenerate_uuids,
        "report": job.report,
        "error": job.error,
        "people_created": job.created_people.iter().flatten().count(),
        "files_restored": job.files_restored,
        "created_at": job.created_at,
        "updated_at": job.updated_at,
        "checked_at": job.checked_at,
        "confirmed_at": job.confirmed_at,
        "applied_at": job.applied_at,
        "finished_at": job.finished_at,
        "reset_at": job.reset_at,
        "pruned_at": job.pruned_at,
        "actions": actions(job),
    })
}

/// The staff accounts the import created, from its report: those an
/// invitation lets sign in (U16).
pub fn created_staff(job: &WorkspaceImportJob) -> Vec<Uuid> {
    let created = job.created();
    let Some(people) = job
        .report
        .as_ref()
        .and_then(|r| r.pointer("/plan/people"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    people
        .iter()
        .filter(|p| p.get("staff").and_then(Value::as_bool) == Some(true))
        .filter(|p| p.get("outcome").and_then(Value::as_str) == Some("created"))
        .filter_map(|p| p.get("target_uuid")?.as_str()?.parse::<Uuid>().ok())
        .filter(|u| created.contains(u))
        .collect()
}

// ---------------------------------------------------------------------------
// Stalled jobs and the prune
// ---------------------------------------------------------------------------

/// Whether a running job has gone silent.
pub fn stalled(job: &WorkspaceImportJob, now: chrono::DateTime<Utc>) -> bool {
    repo::RUNNING.contains(&job.state.as_str())
        && now - job.updated_at
            > chrono::Duration::from_std(STALLED_AFTER).unwrap_or(chrono::Duration::minutes(5))
}

/// Put a stalled job back, under its row lock, in the caller's
/// transaction. Returns the job as it now stands.
pub fn settle(
    conn: &mut DbConnection,
    workspace_id: i32,
    id: Uuid,
) -> QueryResult<Option<WorkspaceImportJob>> {
    let Some(job) = repo::get_for_update(conn, workspace_id, id)? else {
        return Ok(None);
    };
    if !stalled(&job, Utc::now()) {
        return Ok(Some(job));
    }
    let (state, refused) = match (job.state.as_str(), job.applied_at.is_some()) {
        (CHECKING, _) => (
            UPLOADED,
            Refused::new(
                "workspace_import.check_stopped",
                "The check stopped before it finished. Check the export again.",
            ),
        ),
        (_, false) => (
            PREVIEWED,
            Refused::new(
                "workspace_import.import_stopped",
                "The import stopped before it finished. Nothing was imported.",
            ),
        ),
        (_, true) => (
            FAILED,
            Refused::new(
                "files_partial",
                "The rows were imported, but the import stopped while restoring files. Retry the \
                 files.",
            ),
        ),
    };
    tracing::info!(id = %id, "stalled workspace import put back");
    repo::transition(
        conn,
        workspace_id,
        id,
        repo::RUNNING,
        WorkspaceImportJobChange {
            state: Some(state.into()),
            stage: Some(None),
            error: Some(Some(refusal_value(&refused))),
            finished_at: Some((state == FAILED).then(Utc::now)),
            ..Default::default()
        },
    )
}

/// The report with the people taken out, as the prune keeps it: the counts
/// stay.
pub fn pruned_report(report: Option<Value>) -> Option<Value> {
    let mut report = report?;
    if let Some(plan) = report.get_mut("plan").and_then(Value::as_object_mut) {
        plan.remove("people");
    }
    if let Some(obj) = report.as_object_mut() {
        obj.remove(FILE_KEYS);
    }
    Some(report)
}

/// The 30-day prune of job `id`, under its row lock, in the caller's
/// transaction: a job that imported nothing and is still waiting is
/// deleted; any other loses its archive and the people in its report. Returns
/// whether it pruned. The caller deletes the archive after commit.
pub fn prune(
    conn: &mut DbConnection,
    workspace_id: i32,
    id: Uuid,
    before: chrono::DateTime<Utc>,
) -> QueryResult<bool> {
    let Some(job) = repo::get_for_update(conn, workspace_id, id)? else {
        return Ok(false);
    };
    if job.pruned_at.is_some()
        || job.updated_at >= before
        || repo::RUNNING.contains(&job.state.as_str())
    {
        return Ok(false);
    }
    if job.applied_at.is_none() && matches!(job.state.as_str(), UPLOADED | PREVIEWED) {
        return Ok(repo::delete(conn, workspace_id, id)? > 0);
    }
    let from: Vec<&str> = vec![job.state.as_str()];
    let from: &[&str] = &from;
    Ok(repo::transition(
        conn,
        workspace_id,
        id,
        from,
        WorkspaceImportJobChange {
            archive_key: Some(None),
            report: Some(pruned_report(job.report.clone())),
            pruned_at: Some(Some(Utc::now())),
            ..Default::default()
        },
    )?
    .is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(state: &str) -> WorkspaceImportJob {
        let now = Utc::now();
        WorkspaceImportJob {
            id: Uuid::now_v7(),
            workspace_id: 1,
            runner: "owner".into(),
            requested_by: None,
            state: state.into(),
            stage: None,
            archive_key: Some("workspace-imports/x.nosdesk".into()),
            archive_name: None,
            archive_size: 0,
            sealed: false,
            regenerate_uuids: false,
            report: None,
            error: None,
            created_people: Vec::new(),
            files_restored: 0,
            created_at: now,
            updated_at: now,
            checked_at: None,
            confirmed_by: None,
            confirmed_at: None,
            applied_at: None,
            finished_at: None,
            reset_by: None,
            reset_at: None,
            pruned_at: None,
        }
    }

    #[test]
    fn archives_park_outside_the_csv_importers_prefix() {
        let key = archive_key(Uuid::now_v7());
        assert!(key.starts_with(ARCHIVE_PREFIX));
        assert!(!key.starts_with("imports/"));
        assert!(crate::utils::storage::is_safe_storage_path(&key));
        assert!(!crate::services::workspace_export::is_exported_file(&key));
    }

    #[test]
    fn each_state_offers_its_actions() {
        let uploaded = actions(&job(UPLOADED));
        assert_eq!(uploaded["check"], true);
        assert_eq!(uploaded["confirm"], false);
        assert_eq!(uploaded["reset"], true, "an upload can be discarded");

        let previewed = actions(&job(PREVIEWED));
        assert_eq!(previewed["confirm"], true);

        for running in [CHECKING, IMPORTING] {
            let a = actions(&job(running));
            assert_eq!(a["reset"], false, "{running}");
            assert_eq!(a["confirm"], false, "{running}");
        }

        let mut partial = job(FAILED);
        partial.applied_at = Some(Utc::now());
        assert_eq!(actions(&partial)["retry_files"], true);
        assert_eq!(actions(&partial)["reset"], true);
        partial.archive_key = None;
        assert_eq!(actions(&partial)["retry_files"], false, "no archive left");

        let mut imported = job(IMPORTED);
        imported.applied_at = Some(Utc::now());
        assert_eq!(actions(&imported)["reset"], true);
        imported.pruned_at = Some(Utc::now());
        assert_eq!(actions(&imported)["reset"], false, "the window closed");
        assert_eq!(actions(&job(RESET))["reset"], false);
    }

    #[test]
    fn a_silent_running_job_is_stalled() {
        let mut checking = job(CHECKING);
        assert!(!stalled(&checking, Utc::now()));
        checking.updated_at = Utc::now() - chrono::Duration::minutes(6);
        assert!(stalled(&checking, Utc::now()));
        let mut previewed = job(PREVIEWED);
        previewed.updated_at = checking.updated_at;
        assert!(!stalled(&previewed, Utc::now()), "only running jobs stall");
    }

    #[test]
    fn the_prune_keeps_counts_and_drops_people() {
        let report = json!({
            "source": { "nosdesk_version": "1.1.3" },
            "plan": { "counts": { "created": 2 }, "people": [{ "email": "a@example.com" }] },
        });
        let pruned = pruned_report(Some(report)).expect("report");
        assert_eq!(pruned["plan"]["counts"]["created"], 2);
        assert!(pruned["plan"].get("people").is_none());
    }

    #[test]
    fn created_staff_are_the_created_people_marked_staff() {
        let staff = Uuid::now_v7();
        let requester = Uuid::now_v7();
        let mapped = Uuid::now_v7();
        let mut j = job(IMPORTED);
        j.created_people = vec![Some(staff), Some(requester)];
        j.report = Some(json!({ "plan": { "people": [
            { "target_uuid": staff, "staff": true, "outcome": "created" },
            { "target_uuid": requester, "staff": false, "outcome": "created" },
            { "target_uuid": mapped, "staff": true, "outcome": "mapped" },
        ] } }));
        assert_eq!(created_staff(&j), vec![staff]);
    }

    #[test]
    fn retry_files_reads_back_the_keys_the_import_gave_files() {
        let report = json!({ FILE_KEYS: {
            "tickets/7/notes/a.png": "tickets/70/notes/a.png",
            "assets/3/media/b.png": "assets/30/media/b.png",
            "branding/logo.png": "branding/logo.png",
        } });
        let keys = StoredKeys::from_report(&report);
        assert_eq!(keys.key(Entity::Ticket, "7").as_deref(), Some("70"));
        assert_eq!(keys.key(Entity::Asset, "3").as_deref(), Some("30"));
        assert_eq!(keys.key(Entity::Ticket, "8"), None);
        let pruned = pruned_report(Some(report)).expect("report");
        assert!(pruned.get(FILE_KEYS).is_none());
    }

    #[test]
    fn version_refusals_carry_their_codes() {
        use crate::services::workspace_import::ImportRefusal;
        let r = read_refused(BackupError::NotImportable(ImportRefusal::Newer {
            export_version: "9.0.0".into(),
        }));
        assert_eq!(r.code, "workspace_import.newer_version");
        assert_eq!(r.args["export_version"], "9.0.0");
        let r = read_refused(BackupError::InvalidPassword);
        assert_eq!(r.code, "workspace_import.password_wrong");
    }
}
