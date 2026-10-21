//! Applying an import plan to an existing workspace, and resetting a
//! workspace back to how it was provisioned.
//!
//! [`import_into`] fills a workspace nothing has been added to (self-hosted:
//! workspace 1; hosted: the workspace the control plane provisioned). In one
//! transaction it takes the workspace's import lock and locks its row, checks
//! the workspace is untouched ([`untouched`]), plans the import
//! ([`plan::plan_import`]), deletes the seeded configuration the archive
//! replaces, loads the plan's people and rows in batches, applies the
//! archive's workspace settings, and bumps the workspace's data epoch as the
//! last write so every client drops its cache. The workspace keeps its own
//! id, uuid, slug, name, plan, seat limit and custom domain (the `workspaces`
//! row only takes the archive's `settings`), and on hosted its portal name.
//! It takes the archive's branding images at its own address, and keeps its
//! own for any the archive doesn't bring ([`files::settle_branding`]).
//!
//! Rows go in one transaction: the archive is already held in memory whole
//! (the upload cap bounds it), so a load that fails anywhere leaves the
//! workspace exactly as it was. Files can't join that transaction, so the
//! import maps them ([`files::FilePlan`]) and the caller restores them after
//! commit ([`files::restore`]): a failure there is resumed by running the
//! restore again with the same plan, or undone with [`reset`].
//!
//! Both [`import_into`] and [`reset`] run on the owner role (`nosdesk_admin`,
//! through `with_actor_bypass_context` or `PlatformConn`), and refuse any
//! other: `workspaces` and `asset_disposals` take updates and deletes only
//! there. Every statement filters by the target workspace explicitly, since
//! row-level security doesn't scope that role. `nosdesk.in_audit_read` keeps
//! the audit triggers out of the bulk load and the clear, as the import
//! always has.

use std::collections::HashSet;
use std::sync::Arc;

use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Bool, Integer, Text};
use serde::Serialize;
use uuid::Uuid;

use super::files::{self, FilePlan};
use super::plan::{self, FkEdge, ImportPlan, ImportReport, ImportTarget, PlanOptions, Row};
use super::policy::{self, Target};
use super::rows;
use crate::db::DbConnection;
use crate::handlers::branding::BRANDING_DIR;
use crate::middleware::workspace_context::DeploymentMode;
use crate::repository;
use crate::repository::workspaces::DataFence;
use crate::services::backup::{table_exists_in_db, BackupError};
use crate::services::workspace_export::{all_workspace_tables, is_exported_file};
use crate::services::workspace_import::{ArchiveContents, ImportRefusal};
use crate::utils::storage::{Storage, StorageError, WorkspaceScopedStorage};

/// Rows per insert statement.
const LOAD_BATCH: usize = 500;

/// Collaborative-document snapshots: state the target rebuilds. Every
/// document in the workspace is replaced, so they go too.
const SNAPSHOTS: &str = "yjs_snapshots";

/// What the untouched check found, as `what` (a table, `documentation_pages`
/// outside the seeded collection, or `files`) and how many.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Found {
    pub what: String,
    pub rows: i64,
}

/// Why an import into, or a reset of, a workspace didn't happen.
#[derive(Debug)]
pub enum ApplyError {
    /// No live workspace has this id.
    NoWorkspace {
        workspace_id: i32,
    },
    /// Another import or reset of the workspace is running.
    Busy {
        workspace_id: i32,
    },
    /// The workspace holds data of its own.
    NotUntouched {
        found: Vec<Found>,
    },
    /// The archive can't be imported (the plan's refusals).
    Refused(ImportRefusal),
    /// The connection isn't on the owner role.
    NotElevated,
    Failed(BackupError),
}

impl ApplyError {
    /// The code a client words the refusal by.
    pub fn code(&self) -> &'static str {
        match self {
            ApplyError::NoWorkspace { .. } => "workspace_not_found",
            ApplyError::Busy { .. } => "import_in_progress",
            ApplyError::NotUntouched { .. } => "workspace_not_empty",
            ApplyError::Refused(_) => "import_refused",
            ApplyError::NotElevated | ApplyError::Failed(_) => "import_failed",
        }
    }
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::NoWorkspace { workspace_id } => {
                write!(f, "There's no workspace {workspace_id} to import into.")
            }
            ApplyError::Busy { .. } => f.write_str(
                "An import or reset of this workspace is already running. Try again when it \
                 finishes.",
            ),
            ApplyError::NotUntouched { found } => {
                let held: Vec<String> = found
                    .iter()
                    .map(|f| format!("{} {}", f.rows, f.what.replace('_', " ")))
                    .collect();
                write!(
                    f,
                    "This workspace already has data ({}). An export imports only into a \
                     workspace nothing has been added to.",
                    held.join(", ")
                )
            }
            ApplyError::Refused(refusal) => write!(f, "{refusal}"),
            ApplyError::NotElevated => {
                f.write_str("The import needs the database owner role and doesn't have it.")
            }
            ApplyError::Failed(e) => write!(f, "{e}"),
        }
    }
}

impl From<BackupError> for ApplyError {
    fn from(e: BackupError) -> Self {
        match e {
            BackupError::NotImportable(refusal) => ApplyError::Refused(refusal),
            e => ApplyError::Failed(e),
        }
    }
}

impl From<diesel::result::Error> for ApplyError {
    fn from(e: diesel::result::Error) -> Self {
        ApplyError::Failed(BackupError::DatabaseError(e))
    }
}

/// A completed import into an existing workspace.
#[derive(Debug)]
pub struct Applied {
    pub workspace_id: i32,
    pub tables_imported: usize,
    pub rows_imported: i64,
    /// The target's rows the import deleted first (seeded configuration).
    pub cleared: Vec<Found>,
    pub fence: DataFence,
    /// The plan's report: per-table counts, the people map, handover items.
    pub report: ImportReport,
    /// Where the archive's files land, for [`files::restore`].
    pub files: FilePlan,
}

/// A completed reset.
#[derive(Debug)]
pub struct Reset {
    pub workspace_id: i32,
    /// The rows the reset deleted, by table.
    pub cleared: Vec<Found>,
    pub fence: DataFence,
}

// ---------------------------------------------------------------------------
// Untouched
// ---------------------------------------------------------------------------

/// The workspace files (workspace-relative paths) that are the workspace's
/// own data: everything an export would carry but its branding images,
/// which are configuration an import replaces and a reset keeps. Earlier
/// exports, CSV import uploads, staged uploads and waiting archives don't
/// count.
pub fn data_files(paths: &[String]) -> Vec<String> {
    let branding = format!("{BRANDING_DIR}/");
    paths
        .iter()
        .filter(|p| is_exported_file(p) && !p.starts_with(&branding))
        .cloned()
        .collect()
}

/// The workspace's data files in `storage` ([`data_files`]), for
/// [`untouched`] and [`import_into`], which can't read storage inside their
/// transaction.
pub async fn held_files(
    storage: Arc<dyn Storage>,
    workspace_id: i32,
) -> Result<Vec<String>, StorageError> {
    let scoped = WorkspaceScopedStorage::arc(storage, workspace_id);
    Ok(data_files(&scoped.list_prefix("").await?))
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn count(conn: &mut DbConnection, query: &str, workspace_id: i32) -> QueryResult<i64> {
    sql_query(query)
        .bind::<Integer, _>(workspace_id)
        .get_result::<Count>(conn)
        .map(|c| c.n)
}

/// What the workspace holds beyond what provisioning gave it (design §2.2):
/// rows in any table that must be empty, documentation pages outside the
/// seeded system collections, and data files (`held_files`, from
/// [`held_files`]). Empty means untouched. Seeded and edited configuration,
/// members and their per-person rows don't count. Run it elevated: every
/// count filters by the workspace itself.
pub fn untouched(
    conn: &mut DbConnection,
    workspace_id: i32,
    held_files: &[String],
) -> Result<Vec<Found>, BackupError> {
    let mut tables = all_workspace_tables(conn)?;
    tables.sort_by_key(|t| (t != "tickets", t.clone()));
    let mut found = Vec::new();
    for table in tables {
        if policy::target(&table) != Target::MustBeEmpty
            || policy::FOLLOW_SEEDED_DOCUMENTS.contains(&table.as_str())
        {
            continue;
        }
        let rows = count(
            conn,
            &format!(
                "SELECT count(*)::bigint AS n FROM public.\"{table}\" WHERE workspace_id = $1"
            ),
            workspace_id,
        )
        .map_err(BackupError::DatabaseError)?;
        if rows > 0 {
            found.push(Found { what: table, rows });
        }
    }
    let pages = count(
        conn,
        "SELECT count(*)::bigint AS n FROM public.documentation_pages p \
         WHERE p.workspace_id = $1 AND NOT EXISTS ( \
           SELECT 1 FROM public.documentation_collection_pages cp \
           JOIN public.documentation_collections c ON c.id = cp.collection_id \
           WHERE cp.page_id = p.id AND c.is_system)",
        workspace_id,
    )
    .map_err(BackupError::DatabaseError)?;
    if pages > 0 {
        found.push(Found {
            what: "documentation_pages".into(),
            rows: pages,
        });
    }
    let files = data_files(held_files).len() as i64;
    if files > 0 {
        found.push(Found {
            what: "files".into(),
            rows: files,
        });
    }
    Ok(found)
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// The connection must be on the owner role: it updates `workspaces` and may
/// delete `asset_disposals`, which the app role can't.
fn require_owner_role(conn: &mut DbConnection) -> Result<(), ApplyError> {
    #[derive(QueryableByName)]
    struct Flag {
        #[diesel(sql_type = Bool)]
        ok: bool,
    }
    let flag: Flag = sql_query(
        "SELECT (r.rolbypassrls OR r.rolsuper) \
           AND has_table_privilege('public.workspaces', 'UPDATE') \
           AND has_table_privilege('public.asset_disposals', 'DELETE') AS ok \
         FROM pg_roles r WHERE r.rolname = current_user",
    )
    .get_result(conn)?;
    if flag.ok {
        Ok(())
    } else {
        Err(ApplyError::NotElevated)
    }
}

/// Lock and check the target for the rest of the transaction: the import
/// lock (another import or reset refuses rather than waits), then the row.
/// Returns the workspace's uuid.
fn lock_workspace(conn: &mut DbConnection, workspace_id: i32) -> Result<Uuid, ApplyError> {
    if !repository::workspaces::try_lock_import(conn, workspace_id)? {
        return Err(ApplyError::Busy { workspace_id });
    }
    repository::workspaces::lock_for_import(conn, workspace_id)?
        .map(|w| w.uuid)
        .ok_or(ApplyError::NoWorkspace { workspace_id })
}

/// Import `contents` into the existing, untouched workspace `target`. Run it
/// on the owner role ([`ApplyError::NotElevated`] otherwise). `held_files` is
/// the workspace's data files ([`held_files`]); any refuses the import as
/// rows do. Atomic: on any error nothing is written. The caller restores the
/// archive's files afterwards ([`files::restore`] with [`Applied::files`]).
pub fn import_into(
    conn: &mut DbConnection,
    contents: &ArchiveContents,
    target: &ImportTarget,
    options: PlanOptions,
    held_files: &[String],
) -> Result<Applied, ApplyError> {
    let workspace_id = target.workspace_id;
    conn.transaction::<_, ApplyError, _>(|conn| {
        require_owner_role(conn)?;
        sql_query("SET LOCAL nosdesk.in_audit_read = 'true'").execute(conn)?;
        let workspace_uuid = lock_workspace(conn, workspace_id)?;

        let found = untouched(conn, workspace_id, held_files)?;
        if !found.is_empty() {
            return Err(ApplyError::NotUntouched { found });
        }

        let mut plan = plan::plan_import(conn, contents, target, options)?;
        if !plan.importable() {
            return Err(ApplyError::Refused(ImportRefusal::Plan(
                plan.report.refusals.clone(),
            )));
        }
        let alloc = plan.allocate(conn)?;
        let files = FilePlan::build(contents, &alloc.lookup());

        let kept = repository::site_settings::kept_branding(
            conn,
            workspace_id,
            target.mode == DeploymentMode::Hosted,
        )?;
        let cleared = clear(conn, workspace_id, |class| class == Target::Replace)?;
        let (tables_imported, rows_imported) = load(conn, &mut plan, &alloc)?;
        files::settle_branding(
            conn,
            workspace_id,
            workspace_uuid,
            contents,
            &files,
            kept.as_ref(),
        )?;
        if let Some(settings) = &plan.workspace_settings {
            repository::workspaces::merge_settings(conn, workspace_id, settings)?;
        }
        // Last: the fence commits with the data.
        let fence = repository::workspaces::bump_data_epoch(conn, workspace_id)?;

        Ok(Applied {
            workspace_id,
            tables_imported,
            rows_imported,
            cleared,
            fence,
            report: plan.report,
            files,
        })
    })
}

/// Load the plan's new people, their emails, then every table's rows in
/// load order. Returns `(tables, rows)` loaded. A row [`ImportPlan::rows`]
/// leaves out at load (a reference the plan kept that has no new key) is
/// counted in the report with the rows the plan left out for the same
/// reason.
pub(crate) fn load(
    conn: &mut DbConnection,
    plan: &mut ImportPlan,
    alloc: &plan::Allocation,
) -> Result<(usize, i64), BackupError> {
    load_rows(conn, "users", &plan.new_users(), "")?;
    insert_primary_emails(conn, plan)?;
    let mut rows_loaded = 0i64;
    let mut skipped: Vec<(String, usize)> = Vec::new();
    for table in plan.tables() {
        let rows = plan.rows(table, alloc)?;
        if rows.len() < table.row_count() {
            skipped.push((table.name.clone(), table.row_count() - rows.len()));
        }
        let suffix = if table.skip_conflicts {
            "ON CONFLICT DO NOTHING"
        } else {
            ""
        };
        if table.name == "rule_versions" {
            replace_written_rule_versions(conn, &rows)?;
        }
        if table.name == "rules" {
            // The version the insert trigger writes names the session's
            // actor, which may be a system actor with no account; it is
            // replaced by the archive's own below, so it names the rule's
            // creator instead.
            let actor = session_actor(conn)?;
            set_session_actor(conn, "")?;
            load_rows(conn, &table.name, &rows, suffix)?;
            set_session_actor(conn, &actor)?;
        } else {
            load_rows(conn, &table.name, &rows, suffix)?;
        }
        rows_loaded += rows.len() as i64;
    }
    for (name, n) in skipped {
        if let Some(report) = plan.report.tables.iter_mut().find(|t| t.table == name) {
            report.planned = report.planned.saturating_sub(n);
            *report
                .dropped
                .entry(plan::UNRESOLVED_REFERENCE)
                .or_default() += n;
        }
    }
    Ok((plan.tables().len(), rows_loaded))
}

/// The transaction's `app.actor_uuid`, empty when unset.
fn session_actor(conn: &mut DbConnection) -> Result<String, BackupError> {
    #[derive(QueryableByName)]
    struct Setting {
        #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
        value: Option<String>,
    }
    sql_query("SELECT current_setting('app.actor_uuid', true) AS value")
        .get_result::<Setting>(conn)
        .map(|s| s.value.unwrap_or_default())
        .map_err(BackupError::DatabaseError)
}

fn set_session_actor(conn: &mut DbConnection, actor: &str) -> Result<(), BackupError> {
    sql_query("SELECT set_config('app.actor_uuid', $1, true)")
        .bind::<Text, _>(actor)
        .execute(conn)
        .map(|_| ())
        .map_err(BackupError::DatabaseError)
}

/// `rules_version_on_insert` writes version 1 of each rule as the rule loads;
/// the archive carries the rule's own history, version 1 included. Remove
/// the versions the trigger wrote that the archive's rows replace (the rule
/// ids are this import's own, so nothing else matches).
fn replace_written_rule_versions(conn: &mut DbConnection, rows: &[Row]) -> Result<(), BackupError> {
    if rows.is_empty() {
        return Ok(());
    }
    let keys: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| serde_json::json!({"rule_id": r.get("rule_id"), "version": r.get("version")}))
        .collect();
    let payload = serde_json::to_string(&keys).map_err(BackupError::JsonError)?;
    sql_query(
        "DELETE FROM public.rule_versions v \
         USING jsonb_to_recordset($1::jsonb) AS a(rule_id integer, version integer) \
         WHERE v.rule_id = a.rule_id AND v.version = a.version",
    )
    .bind::<Text, _>(payload)
    .execute(conn)
    .map_err(BackupError::DatabaseError)?;
    Ok(())
}

/// Insert `rows` into `table` in batches, through the column policy
/// (`rows::load_statement`): only the columns the rows carry, so the rest take
/// their defaults.
fn load_rows(
    conn: &mut DbConnection,
    table: &str,
    rows: &[Row],
    suffix: &str,
) -> Result<(), BackupError> {
    if rows.is_empty() {
        return Ok(());
    }
    if !table_exists_in_db(conn, table)? {
        return Err(BackupError::CorruptedBackup(format!(
            "archive references unknown table: {table}"
        )));
    }
    for batch in rows.chunks(LOAD_BATCH) {
        let keys: HashSet<String> = batch.iter().flat_map(|r| r.keys().cloned()).collect();
        let query = rows::load_statement(conn, table, &keys, &[], suffix)?;
        let payload = serde_json::to_string(batch).map_err(BackupError::JsonError)?;
        sql_query(&query)
            .bind::<Text, _>(payload)
            .execute(conn)
            .map_err(BackupError::DatabaseError)?;
    }
    Ok(())
}

/// Give each account the plan creates the primary email the archive carries
/// for them (placeholders get none).
///
/// Only the address and its type come from the archive, which is untrusted:
/// the row is unverified with no `source`, whatever the archive says, so an
/// archive can't vouch for an address (`is_verified` is the inbound-mail
/// impersonation guard) or tie it to a sign-in provider. The person proves it
/// by accepting an invitation, as anyone added by an admin does.
fn insert_primary_emails(conn: &mut DbConnection, plan: &ImportPlan) -> Result<(), BackupError> {
    for (owner, address, email_type) in plan.new_emails() {
        sql_query(
            "INSERT INTO public.user_emails \
               (user_uuid, email, email_type, is_primary, is_verified, source, created_by, \
                created_at, updated_at) \
             VALUES ($1, $2, $3, true, false, NULL, \
               (SELECT u.uuid FROM public.users u \
                WHERE u.uuid = NULLIF(current_setting('app.actor_uuid', true), '')::uuid), \
               now(), now())",
        )
        .bind::<diesel::sql_types::Uuid, _>(owner)
        .bind::<Text, _>(address)
        .bind::<Text, _>(email_type)
        .execute(conn)
        .map_err(BackupError::DatabaseError)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Clear and reset
// ---------------------------------------------------------------------------

/// Every foreign key among `tables`, whatever columns it uses, as edges for
/// [`plan::topological_order`].
fn all_edges(conn: &mut DbConnection, tables: &[String]) -> Result<Vec<FkEdge>, BackupError> {
    #[derive(QueryableByName)]
    struct Edge {
        #[diesel(sql_type = Text)]
        child: String,
        #[diesel(sql_type = Text)]
        name: String,
        #[diesel(sql_type = Text)]
        parent: String,
    }
    let set: HashSet<&str> = tables.iter().map(String::as_str).collect();
    let edges: Vec<Edge> = sql_query(
        "SELECT conrelid::regclass::text AS child, conname::text AS name, \
                confrelid::regclass::text AS parent \
         FROM pg_constraint WHERE contype = 'f' AND connamespace = 'public'::regnamespace",
    )
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    Ok(edges
        .into_iter()
        .filter(|e| set.contains(e.child.as_str()) && set.contains(e.parent.as_str()))
        .map(|e| FkEdge {
            child_table: e.child,
            child_column: e.name,
            parent_table: e.parent,
        })
        .collect())
}

/// Delete the workspace's rows in every table whose target class `include`
/// accepts, with the documentation children and snapshots that follow them,
/// children before parents. Returns what it deleted.
fn clear(
    conn: &mut DbConnection,
    workspace_id: i32,
    include: impl Fn(Target) -> bool,
) -> Result<Vec<Found>, BackupError> {
    let tables: Vec<String> = all_workspace_tables(conn)?
        .into_iter()
        .filter(|t| {
            include(policy::target(t))
                || policy::FOLLOW_SEEDED_DOCUMENTS.contains(&t.as_str())
                || t == SNAPSHOTS
        })
        .collect();
    let edges = all_edges(conn, &tables)?;
    let mut order = plan::topological_order(&tables, &edges)?;
    order.reverse();
    let mut cleared = Vec::new();
    for table in order {
        let rows = sql_query(format!(
            "DELETE FROM public.\"{table}\" WHERE workspace_id = $1"
        ))
        .bind::<Integer, _>(workspace_id)
        .execute(conn)
        .map_err(BackupError::DatabaseError)?;
        if rows > 0 {
            cleared.push(Found {
                what: table,
                rows: rows as i64,
            });
        }
    }
    cleared.sort_by(|a, b| a.what.cmp(&b.what));
    Ok(cleared)
}

/// Reset the workspace to how provisioning left it (design §2.4): delete
/// every content and configuration row (an import's, or anything else's),
/// seed the defaults again, and bump the data epoch. Its members and their
/// per-person rows stay, people an import created among them. Branding
/// images stay, and on hosted the portal name. `seed_author` authors the
/// welcome page; without one the page is left for the next seed (startup on
/// self-hosted, owner projection on hosted).
///
/// Run it on the owner role: `asset_disposals` takes deletes only there. The
/// caller deletes the workspace's files ([`clear_files`]) after it commits.
pub fn reset(
    conn: &mut DbConnection,
    workspace_id: i32,
    mode: DeploymentMode,
    seed_author: Option<Uuid>,
) -> Result<Reset, ApplyError> {
    conn.transaction::<_, ApplyError, _>(|conn| {
        require_owner_role(conn)?;
        sql_query("SET LOCAL nosdesk.in_audit_read = 'true'").execute(conn)?;
        lock_workspace(conn, workspace_id)?;

        let kept = repository::site_settings::kept_branding(
            conn,
            workspace_id,
            mode == DeploymentMode::Hosted,
        )?;
        let cleared = clear(conn, workspace_id, |class| class != Target::Keep)?;

        // Seed as provisioning does: the app role pinned to the workspace, so
        // each seed's "already seeded?" read sees only this workspace.
        let seed_actor = crate::sync::actor::ActorContext::system("workspace_import:reset")
            .with_workspace(workspace_id);
        crate::sync::session::set_actor(conn, &seed_actor)?;
        crate::services::seed::seed_workspace_defaults(conn, seed_author)?;
        if let Some(author) = seed_author {
            crate::services::seed::seed_getting_started(conn, author)?;
        }
        // elevated-in-workspace: back on the owner role for this workspace's own `workspaces` row
        // (the data epoch), which nosdesk_app can only read; the seed above ran as the app role.
        sql_query("SET LOCAL ROLE nosdesk_admin").execute(conn)?;

        if let Some(kept) = kept {
            repository::site_settings::restore_kept_branding(conn, workspace_id, &kept)?;
        }
        let fence = repository::workspaces::bump_data_epoch(conn, workspace_id)?;
        Ok(Reset {
            workspace_id,
            cleared,
            fence,
        })
    })
}

/// Delete the workspace's data files ([`data_files`]), after a reset commits.
/// Returns how many it deleted.
pub async fn clear_files(
    storage: Arc<dyn Storage>,
    workspace_id: i32,
) -> Result<usize, StorageError> {
    let scoped = WorkspaceScopedStorage::arc(storage, workspace_id);
    let paths = data_files(&scoped.list_prefix("").await?);
    for path in &paths {
        scoped.delete_file(path).await?;
    }
    Ok(paths.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Files under the prefixes an export leaves out, and branding images,
    /// don't make a workspace touched; anything else does.
    #[test]
    fn only_files_an_export_carries_count_as_data() {
        let paths: Vec<String> = [
            "imports/0198/upload.csv",
            "imports/0198/grid.1.json",
            "exports/workspace-1.nosdesk",
            "workspace-imports/job.nosdesk",
            "temp/staged.png",
            "branding/logo.png",
            "tickets/12/attachments/a.pdf",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(data_files(&paths), vec!["tickets/12/attachments/a.pdf"]);
    }

    #[test]
    fn the_refusal_names_what_it_found() {
        let e = ApplyError::NotUntouched {
            found: vec![
                Found {
                    what: "tickets".into(),
                    rows: 3,
                },
                Found {
                    what: "files".into(),
                    rows: 2,
                },
            ],
        };
        assert_eq!(e.code(), "workspace_not_empty");
        let message = e.to_string();
        assert!(message.contains("3 tickets"), "{message}");
        assert!(message.contains("2 files"), "{message}");
    }
}
