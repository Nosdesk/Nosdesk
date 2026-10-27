//! Workspace export and import from a shell: what `nosdesk-cli workspace`
//! runs, for an operator moving a workspace onto or off a server (Cloud
//! moves included, run on the product machine) and for self-hosters with
//! large archives or scripted moves.
//!
//! An import is a workspace import job (`services::workspace_import_jobs`)
//! with the operator origin, run in this process rather than the server's:
//! parked, checked, confirmed, imported and reset by the functions the app's
//! routes call. It shows in Recent imports, resets like any other job and
//! leaves the same audit trail. A dry run plans the import without a job and
//! writes nothing.
//!
//! An export is the archive the platform admin export builds: sealed with a
//! password (keeping the policy's secret columns) or plaintext without them,
//! and with no size cap.
//!
//! The CLI can't open the server's search index, so after an import or reset
//! it asks the running server to reindex the workspace
//! (`services::search::reindex_requests`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::db::Pool;
use crate::middleware::workspace_context::DeploymentMode;
use crate::models::{Workspace, WorkspaceImportJob};
use crate::repository::workspace_import_jobs as repo;
use crate::services::imports::refusal::Refused;
use crate::services::workspace_export::{build_archive, BuildError};
use crate::services::workspace_import::read_archive;
use crate::services::workspace_import_jobs::{
    self as jobs, in_workspace, Origin, ParkError, ResetError, Runtime, IMPORTED, IMPORTING,
    PREVIEWED,
};
use crate::sync::actor::ActorContext;
use crate::utils::storage::Storage;

/// How often a running import is read back for its progress.
const POLL: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// Why a command stopped: the code, the English and the values, as the app's
/// routes answer them.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub code: String,
    pub message: String,
    pub args: Map<String, Value>,
}

impl Refusal {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            args: Map::new(),
        }
    }

    /// A refusal as a job stores it (`{code, message, args}`).
    pub fn from_value(value: &Value) -> Self {
        Self {
            code: value
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("import_failed")
                .to_string(),
            message: value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("The import failed.")
                .to_string(),
            args: value
                .get("args")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
        }
    }
}

impl From<Refused> for Refusal {
    fn from(r: Refused) -> Self {
        Self {
            code: r.code.to_string(),
            message: r.message,
            args: r.args,
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&render_refusal(self))
    }
}

impl std::error::Error for Refusal {}

fn failed(e: impl std::fmt::Display) -> Refusal {
    Refusal::new("import_failed", e.to_string())
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

/// The origin the CLI runs jobs as: an operator with no account of their own
/// here, so nobody's email merges (`plan::Runner::Operator`).
pub fn operator() -> Origin {
    Origin::Operator { user: None }
}

/// The runtime for a CLI job: the process's storage, and no search index
/// (the server holds it).
pub fn runtime(pool: Pool, storage: Arc<dyn Storage>) -> Runtime {
    Runtime {
        pool,
        storage,
        search: None,
    }
}

/// The live workspace `target` names: its id, else its slug.
pub async fn resolve_workspace(pool: &Pool, target: &str) -> Result<Workspace, Refusal> {
    let target = target.trim().to_string();
    let shown = target.clone();
    let actor = ActorContext::system("cli:workspace");
    let found = in_workspace(pool, actor, move |conn| match target.parse::<i32>() {
        Ok(id) => crate::repository::workspaces::find_by_id(conn, id),
        Err(_) => crate::repository::workspaces::find_by_slug(conn, &target),
    })
    .await
    .map_err(failed)?;
    found.ok_or_else(|| {
        Refusal::new(
            "workspace_not_found",
            format!("There's no workspace {shown} on this server (give its id or slug)."),
        )
    })
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Export workspace `workspace_id` to `out`, sealed with `password` when
/// given. The file is written beside `out` and moved into place once whole,
/// readable by its owner only. Returns its size.
pub async fn export(
    pool: &Pool,
    storage: Arc<dyn Storage>,
    workspace_id: i32,
    password: Option<&str>,
    out: &Path,
) -> Result<u64, Refusal> {
    let archive = build_archive(pool, storage, workspace_id, password, true, None)
        .await
        .map_err(|e| match e {
            BuildError::TooLarge => Refusal::new("export_too_large", e.to_string()),
            BuildError::Failed(e) => Refusal::new("export_failed", e),
        })?;
    let out = out.to_path_buf();
    tokio::task::spawn_blocking(move || write_private(&out, &archive))
        .await
        .map_err(|e| Refusal::new("export_failed", e.to_string()))?
        .map_err(|e| Refusal::new("export_not_written", e.to_string()))
}

fn write_private(out: &Path, bytes: &[u8]) -> std::io::Result<u64> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut partial = out.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&partial)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&partial, out)?;
    Ok(bytes.len() as u64)
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// An archive on local disk to import into a workspace.
#[derive(Debug, Clone)]
pub struct ImportRequest {
    pub workspace_id: i32,
    pub archive: PathBuf,
    /// Opens a sealed archive. Never stored.
    pub password: Option<String>,
    /// Give every imported row a new uuid: a copy of a workspace into the
    /// server that still holds it.
    pub regenerate_uuids: bool,
}

/// What the import will do: the source and the plan (`{source, plan}`), and,
/// when the plan refuses it, why.
#[derive(Debug, Clone)]
pub struct Preview {
    pub report: Value,
    pub refused: Option<Refusal>,
}

fn archive_head(path: &Path) -> Result<(bool, u64), Refusal> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| {
        Refusal::new(
            "workspace_import.no_file",
            format!("{} can't be read: {e}", path.display()),
        )
    })?;
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut head = [0u8; 4];
    let n = f.read(&mut head).unwrap_or(0);
    Ok((jobs::is_sealed(&head[..n]), size))
}

async fn refuse_if_live(rt: &Runtime, origin: &Origin, workspace_id: i32) -> Result<(), Refusal> {
    let actor = actor(origin, workspace_id);
    let live = in_workspace(&rt.pool, actor, move |conn| repo::live(conn, workspace_id))
        .await
        .map_err(failed)?;
    match live {
        Some(live) => Err(jobs::in_progress(&live).into()),
        None => Ok(()),
    }
}

fn actor(origin: &Origin, workspace_id: i32) -> ActorContext {
    match origin.user() {
        Some(user) => ActorContext::user(user, None).with_workspace(workspace_id),
        None => ActorContext::system("cli:workspace_import").with_workspace(workspace_id),
    }
}

/// Plan the import of `req` as a job's check would, without a job: nothing
/// is written. Refuses as the check does (the workspace not untouched, an
/// export from another version, a wrong password), and when the workspace
/// already has an import under way.
pub async fn dry_run(
    rt: &Runtime,
    origin: &Origin,
    req: &ImportRequest,
) -> Result<Preview, Refusal> {
    refuse_if_live(rt, origin, req.workspace_id).await?;
    let (sealed, _) = archive_head(&req.archive)?;
    if sealed && req.password.is_none() {
        return Err(jobs::password_required().into());
    }
    let path = req.archive.clone();
    let password = req.password.clone();
    let contents = tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&path).map_err(|e| {
            Refusal::new(
                "workspace_import.no_file",
                format!("{} can't be read: {e}", path.display()),
            )
        })?;
        read_archive(&bytes, password.as_deref()).map_err(|e| Refusal::from(jobs::read_refused(e)))
    })
    .await
    .map_err(failed)??;
    let checked = jobs::preview(
        rt,
        origin,
        req.workspace_id,
        Arc::new(contents),
        req.regenerate_uuids,
    )
    .await
    .map_err(Refusal::from)?;
    Ok(Preview {
        report: checked.report,
        refused: checked.refused.map(Refusal::from),
    })
}

/// Start a job for `req` and check it: the archive is parked in the
/// workspace's storage and planned. A previewed job comes back with its
/// preview. A refused one is discarded, so it doesn't hold the workspace's
/// one live import, and comes back `reset` with the preview when the plan
/// was what refused, else as the refusal.
pub async fn check(
    rt: &Runtime,
    origin: &Origin,
    req: &ImportRequest,
) -> Result<(WorkspaceImportJob, Preview), Refusal> {
    let workspace_id = req.workspace_id;
    let (sealed, size) = archive_head(&req.archive)?;
    if sealed && req.password.is_none() {
        return Err(jobs::password_required().into());
    }
    let parked = jobs::park(
        rt,
        origin,
        workspace_id,
        jobs::Upload {
            path: &req.archive,
            name: req
                .archive
                .file_name()
                .map(|n| n.to_string_lossy().into_owned()),
            size,
            password_given: req.password.is_some(),
            regenerate_uuids: req.regenerate_uuids,
        },
    )
    .await
    .map_err(|e| match e {
        ParkError::InProgress(live) => jobs::in_progress(&live).into(),
        ParkError::NotStored => Refusal::new(
            "upload_not_saved",
            "The export couldn't be stored in the workspace's storage.",
        ),
        ParkError::Failed(e) => failed(e),
    })?;
    let id = parked.id;
    jobs::run_check(
        rt,
        origin,
        workspace_id,
        id,
        req.password.clone(),
        req.regenerate_uuids,
    )
    .await;
    let job = read_job(rt, origin, workspace_id, id).await?;
    if job.state == PREVIEWED {
        let preview = Preview {
            report: job.report.clone().unwrap_or(Value::Null),
            refused: None,
        };
        return Ok((job, preview));
    }
    let refusal = job
        .error
        .as_ref()
        .map(Refusal::from_value)
        .unwrap_or_else(|| failed("The check didn't finish."));
    let report = job.report.clone();
    let job = discard(rt, origin, workspace_id, id).await?;
    match report {
        Some(report) => Ok((
            job,
            Preview {
                report,
                refused: Some(refusal),
            },
        )),
        None => Err(refusal),
    }
}

/// Confirm previewed job `id` and run its import to the end: rows, search,
/// files. `progress` sees the job each time its stage changes. Returns the
/// job as it ended: `imported`, or `failed` (its files, to retry) or back to
/// `uploaded` or `previewed` with the refusal in `error`.
pub async fn import(
    rt: &Runtime,
    origin: &Origin,
    workspace_id: i32,
    id: Uuid,
    password: Option<String>,
    progress: impl FnMut(&WorkspaceImportJob),
) -> Result<WorkspaceImportJob, Refusal> {
    let change = jobs::confirming(origin);
    run(
        rt,
        origin,
        workspace_id,
        id,
        password,
        (PREVIEWED, change, false),
        progress,
    )
    .await
}

/// Retry the files of job `id`, whose rows committed and whose files
/// stopped part way (`failed` with `files_partial`). Files already in place
/// are left alone. Returns the job as it ended, as [`import`] does.
pub async fn retry_files(
    rt: &Runtime,
    origin: &Origin,
    workspace_id: i32,
    id: Uuid,
    password: Option<String>,
    progress: impl FnMut(&WorkspaceImportJob),
) -> Result<WorkspaceImportJob, Refusal> {
    let job = read_job(rt, origin, workspace_id, id).await?;
    if jobs::actions(&job)["retry_files"] != true {
        return Err(Refusal::new(
            "workspace_import.no_files_to_retry",
            "This import has no files left to restore.",
        ));
    }
    if job.sealed && password.is_none() {
        return Err(jobs::password_required().into());
    }
    let change = crate::models::WorkspaceImportJobChange {
        state: Some(IMPORTING.into()),
        stage: Some(Some("files".into())),
        finished_at: Some(None),
        ..Default::default()
    };
    run(
        rt,
        origin,
        workspace_id,
        id,
        password,
        (jobs::FAILED, change, true),
        progress,
    )
    .await
}

/// Move job `id` from `from` with `change` (into `importing`), run the
/// import (`files_only` for a retry) and follow it to the end.
async fn run(
    rt: &Runtime,
    origin: &Origin,
    workspace_id: i32,
    id: Uuid,
    password: Option<String>,
    (from, change, files_only): (&'static str, crate::models::WorkspaceImportJobChange, bool),
    mut progress: impl FnMut(&WorkspaceImportJob),
) -> Result<WorkspaceImportJob, Refusal> {
    let moved = in_workspace(&rt.pool, actor(origin, workspace_id), move |conn| {
        repo::transition(conn, workspace_id, id, &[from], change)
    })
    .await
    .map_err(failed)?;
    let Some(job) = moved else {
        let job = read_job(rt, origin, workspace_id, id).await?;
        return Err(Refusal::new(
            "workspace_import.wrong_state",
            format!("This import is {} and can't do that now.", job.state),
        ));
    };
    progress(&job);
    let mut stage = job.stage.clone();
    let run = jobs::spawn_import(
        rt.clone(),
        origin.clone(),
        workspace_id,
        id,
        password,
        files_only,
    );
    while !run.is_finished() {
        tokio::time::sleep(POLL).await;
        if let Ok(job) = read_job(rt, origin, workspace_id, id).await {
            if job.state == IMPORTING && job.stage != stage {
                stage = job.stage.clone();
                progress(&job);
            }
        }
    }
    run.await.map_err(failed)?;
    read_job(rt, origin, workspace_id, id).await
}

/// Reset job `id`: an import that committed rows takes the workspace back to
/// how it was provisioned and removes the people it created; one that
/// imported nothing is discarded.
pub async fn reset(
    rt: &Runtime,
    origin: &Origin,
    workspace_id: i32,
    id: Uuid,
) -> Result<WorkspaceImportJob, Refusal> {
    jobs::reset(
        rt,
        origin,
        workspace_id,
        id,
        DeploymentMode::current(),
        None,
    )
    .await
    .map_err(|e| match e {
        ResetError::NotFound => Refusal::new(
            "workspace_import.not_found",
            format!("There's no workspace import {id}."),
        ),
        ResetError::Refused { refused, .. } => refused.into(),
        ResetError::Failed(e) => failed(e),
    })
}

/// [`reset`] of a job that imported nothing.
async fn discard(
    rt: &Runtime,
    origin: &Origin,
    workspace_id: i32,
    id: Uuid,
) -> Result<WorkspaceImportJob, Refusal> {
    reset(rt, origin, workspace_id, id).await
}

async fn read_job(
    rt: &Runtime,
    origin: &Origin,
    workspace_id: i32,
    id: Uuid,
) -> Result<WorkspaceImportJob, Refusal> {
    in_workspace(&rt.pool, actor(origin, workspace_id), move |conn| {
        repo::get(conn, workspace_id, id)
    })
    .await
    .map_err(failed)?
    .ok_or_else(|| {
        Refusal::new(
            "workspace_import.not_found",
            format!("There's no workspace import {id}."),
        )
    })
}

/// Job `id`, in whichever workspace holds it, put back first if its run
/// died.
pub async fn find_job(pool: &Pool, id: Uuid) -> Result<WorkspaceImportJob, Refusal> {
    let lookup = pool.clone();
    let found = tokio::task::spawn_blocking(move || {
        // cross-tenant: the operator names a job, not its workspace; this reads which workspace holds it, and everything after runs pinned to that one.
        crate::sync::session::background_run(&lookup, "cli:workspace_import_find", |conn| {
            repo::find(conn, id)
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(failed)?
    .map_err(failed)?;
    let Some(job) = found else {
        return Err(Refusal::new(
            "workspace_import.not_found",
            format!("There's no workspace import {id}."),
        ));
    };
    let workspace_id = job.workspace_id;
    let actor = ActorContext::system("cli:workspace_import").with_workspace(workspace_id);
    in_workspace(pool, actor, move |conn| {
        jobs::settle(conn, workspace_id, id)
    })
    .await
    .map_err(failed)?
    .ok_or_else(|| {
        Refusal::new(
            "workspace_import.not_found",
            format!("There's no workspace import {id}."),
        )
    })
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn n(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn arr<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn args_line(args: &Map<String, Value>) -> String {
    args.iter()
        .filter(|(k, _)| k.as_str() != "refusals")
        .map(|(k, v)| match v {
            Value::String(s) => format!("{k}={s}"),
            v => format!("{k}={v}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A refusal in readable form: the message, its code and values, and each
/// plan refusal under it.
pub fn render_refusal(r: &Refusal) -> String {
    let mut out = format!("{} [{}]", r.message, r.code);
    let args = args_line(&r.args);
    if !args.is_empty() {
        out.push_str(&format!(" ({args})"));
    }
    if let Some(found) = r.args.get("found").and_then(Value::as_array) {
        for f in found {
            out.push_str(&format!("\n  holds {} {}", n(f, "rows"), s(f, "what")));
        }
    }
    for each in r
        .args
        .get("refusals")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let inner = Refusal::from_value(each);
        let args = args_line(&inner.args);
        out.push_str(&format!("\n  - {} [{}]", inner.message, inner.code));
        if !args.is_empty() {
            out.push_str(&format!(" ({args})"));
        }
    }
    out
}

fn person_line(p: &Value) -> String {
    let name = s(p, "name");
    let email = p
        .get("email")
        .and_then(Value::as_str)
        .map(|e| format!(" <{e}>"))
        .unwrap_or_default();
    let role = p
        .get("role_on_target")
        .and_then(Value::as_str)
        .map(|r| format!(", {r}"))
        .unwrap_or_else(|| ", no membership".to_string());
    let outcome = match s(p, "outcome") {
        "created" => "new account".to_string(),
        "mapped" => format!("existing account {}", s(p, "target_uuid")),
        "merged_into_runner" => "your own account".to_string(),
        "placeholder" => format!(
            "placeholder ({})",
            p.get("placeholder")
                .and_then(Value::as_str)
                .unwrap_or("no seat")
        ),
        other => other.to_string(),
    };
    let mut notes = Vec::new();
    if p.get("disabled").and_then(Value::as_bool) == Some(true) {
        notes.push("removed in the export");
    }
    if p.get("username_dropped").and_then(Value::as_bool) == Some(true) {
        notes.push("username taken here, dropped");
    }
    if p.get("uuid_reminted").and_then(Value::as_bool) == Some(true) {
        notes.push("new uuid");
    }
    let notes = if notes.is_empty() {
        String::new()
    } else {
        format!(" [{}]", notes.join("; "))
    };
    format!("{name}{email}: {outcome}{role}{notes}")
}

/// The preview of a check or a dry run (`{source, plan}`) in readable form:
/// the export, the rows per table, the people and where each lands, what the
/// import leaves to redo, and the plan's refusals.
pub fn render_preview(report: &Value) -> String {
    let mut out = String::new();
    let source = report.get("source").unwrap_or(&Value::Null);
    let plan = report.get("plan").unwrap_or(&Value::Null);

    out.push_str("Export\n");
    out.push_str(&format!(
        "  from:     Nosdesk {} ({}), workspace \"{}\"",
        s(source, "nosdesk_version"),
        s(source, "mode").replace('_', "-"),
        s(source, "workspace_slug")
    ));
    if let Some(origin) = source.get("origin").and_then(Value::as_str) {
        out.push_str(&format!(" at {origin}"));
    }
    out.push('\n');
    out.push_str(&format!("  made:     {}\n", s(source, "created_at")));
    let sealed = source.get("sealed").and_then(Value::as_bool) == Some(true);
    let secrets = source.get("secrets").and_then(Value::as_bool) == Some(true);
    out.push_str(&format!(
        "  sealed:   {}\n",
        match (sealed, secrets) {
            (true, true) => "yes, with integration secrets",
            (true, false) => "yes",
            _ => "no (integration secrets and MFA aren't carried)",
        }
    ));
    out.push_str(&format!("  files:    {}\n", n(source, "files")));

    let tables: Vec<&Value> = arr(plan, "tables")
        .iter()
        .filter(|t| n(t, "in_archive") > 0)
        .collect();
    out.push_str("\nRows (in the export, to import)\n");
    if tables.is_empty() {
        out.push_str("  none\n");
    }
    let width = tables
        .iter()
        .map(|t| s(t, "table").len())
        .max()
        .unwrap_or(0);
    for t in &tables {
        out.push_str(&format!(
            "  {:<width$}  {:>7}  {:>7}",
            s(t, "table"),
            n(t, "in_archive"),
            n(t, "planned"),
        ));
        let mut left: Vec<String> = t
            .get("dropped")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(why, count)| format!("{} left out: {why}", count.as_u64().unwrap_or(0)))
            .collect();
        left.extend(
            t.get("cleared")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .map(|(col, count)| format!("{} {col} cleared", count.as_u64().unwrap_or(0))),
        );
        if !left.is_empty() {
            out.push_str(&format!("  ({})", left.join("; ")));
        }
        out.push('\n');
    }

    let counts = plan.get("counts").unwrap_or(&Value::Null);
    let people = arr(plan, "people");
    out.push_str(&format!(
        "\nPeople ({}): {} new, {} mapped to existing accounts, {} placeholders\n",
        people.len(),
        n(counts, "created"),
        n(counts, "mapped") + n(counts, "merged_into_runner"),
        n(counts, "placeholders"),
    ));
    for p in people {
        out.push_str(&format!("  {}\n", person_line(p)));
    }

    let handover = arr(plan, "handover");
    if !handover.is_empty() {
        out.push_str("\nTo redo after the import\n");
        for h in handover {
            let line = match s(h, "kind") {
                "encrypted_setting_dropped" => format!(
                    "{}.{}: {} saved credentials left out, enter them again",
                    s(h, "table"),
                    s(h, "column"),
                    n(h, "rows")
                ),
                "tokens_regenerated" => format!(
                    "{}.{}: {} lookup tokens renewed, earlier links stop working",
                    s(h, "table"),
                    s(h, "column"),
                    n(h, "rows")
                ),
                "plugin_secrets_dropped" => {
                    format!("{} plugin secrets left out, enter them again", n(h, "rows"))
                }
                "switched_off" => format!(
                    "{}.{}: {} arrive switched off, turn them back on",
                    s(h, "table"),
                    s(h, "column"),
                    n(h, "rows")
                ),
                other => format!("{other}: {h}"),
            };
            out.push_str(&format!("  {line}\n"));
        }
    }

    let references = arr(plan, "references");
    if !references.is_empty() {
        out.push_str("\nReferences the import can't keep\n");
        for r in references {
            out.push_str(&format!(
                "  {}.{}: {} {}\n",
                s(r, "table"),
                s(r, "column"),
                n(r, "count"),
                s(r, "effect").replace('_', " ")
            ));
        }
    }

    let refusals = arr(plan, "refusals");
    if !refusals.is_empty() {
        out.push_str("\nRefusals\n");
        for r in refusals {
            let code = s(r, "code");
            let args: Map<String, Value> = r
                .as_object()
                .map(|o| {
                    o.iter()
                        .filter(|(k, _)| k.as_str() != "code")
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                })
                .unwrap_or_default();
            out.push_str(&format!(
                "  workspace_import.{code} ({})\n",
                args_line(&args)
            ));
        }
    }
    out
}

/// A job in readable form: its state and, once it imported, what it did:
/// rows, people created, files restored and skipped, and any error.
pub fn render_job(job: &WorkspaceImportJob) -> String {
    let mut out = String::new();
    out.push_str(&format!("Import {}\n", job.id));
    out.push_str(&format!("  workspace: {}\n", job.workspace_id));
    out.push_str(&format!("  runner:    {}\n", job.runner));
    let mut state = job.state.clone();
    if let Some(stage) = &job.stage {
        state.push_str(&format!(" ({stage})"));
    }
    out.push_str(&format!("  state:     {state}\n"));
    if let Some(name) = &job.archive_name {
        out.push_str(&format!(
            "  archive:   {name}, {} bytes{}\n",
            job.archive_size,
            if job.sealed { ", sealed" } else { "" }
        ));
    }
    out.push_str(&format!("  started:   {}\n", job.created_at));
    if let Some(at) = job.applied_at {
        out.push_str(&format!("  imported:  {at}\n"));
    }
    if let Some(at) = job.reset_at {
        out.push_str(&format!("  reset:     {at}\n"));
    }
    if let Some(error) = &job.error {
        out.push_str(&format!(
            "  error:     {}\n",
            render_refusal(&Refusal::from_value(error))
        ));
    }
    let report = job.report.as_ref().unwrap_or(&Value::Null);
    if job.applied_at.is_some() {
        out.push_str(&format!(
            "\nRows imported: {} across {} tables\n",
            n(report, "rows_imported"),
            n(report, "tables_imported")
        ));
        let created: Vec<&Value> = report
            .pointer("/plan/people")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|p| matches!(s(p, "outcome"), "created" | "placeholder"))
            .collect();
        out.push_str(&format!(
            "People created: {}\n",
            job.created_people.iter().flatten().count()
        ));
        for p in created {
            out.push_str(&format!("  {}\n", person_line(p)));
        }
        if let Some(files) = report.get("files") {
            out.push_str(&format!(
                "Files: {} restored, {} already in place, {} skipped\n",
                n(files, "restored"),
                n(files, "present"),
                arr(files, "skipped").len()
            ));
            for f in arr(files, "skipped") {
                let detail = f
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(|d| format!(": {d}"))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "  {} ({}{detail})\n",
                    s(f, "path"),
                    s(f, "reason").replace('_', " ")
                ));
            }
        }
        if let Some(reset) = report.get("reset") {
            out.push_str(&format!(
                "Reset: {} people removed, {} kept (another workspace holds them)\n",
                n(reset, "people_removed"),
                n(reset, "people_kept")
            ));
        }
    }
    if job.state == IMPORTED {
        out.push_str(
            "\nSearch: the running server was asked to reindex the workspace. If it wasn't \
             running, rebuild it with POST /api/search/rebuild?workspace_id=",
        );
        out.push_str(&format!("{}.\n", job.workspace_id));
    }
    out
}
