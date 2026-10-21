//! Workspace-scoped export — Phase 1 of the tenant export/import primitive.
//!
//! Dumps a single workspace's data (the `workspace_id`-scoped tables) plus the
//! `workspaces` row and the workspace's member `users`, into the same
//! encrypted-zip envelope the whole-DB backup uses. It reuses backup.rs's
//! crypto, integrity, and sensitive-field machinery; only the table discovery
//! and the per-row scoping are workspace-specific.
//!
//! Dual-use: per-workspace backup, GDPR data-export / portability, tenant
//! offboarding, and the input to the Phase 2 id-remapping import (region
//! migration). It is read-only, so it cannot corrupt data. It must run
//! privileged (`nosdesk_admin`, BYPASSRLS) via `with_actor_bypass_context` so
//! the cross-table reads see the workspace's rows and the global `workspaces`
//! row.
//!
//! Row collection ([`collect_workspace_rows`]) is synchronous (DB only, runs in
//! the BYPASSRLS transaction); the workspace's uploaded files are read separately
//! through the storage abstraction (async, so it works for local and S3 alike)
//! and folded into the archive under `files/{logical}` by
//! [`assemble_workspace_archive`]. The handler orchestrates the two.
//!
//! Which tables it carries is the workspace archive policy
//! (`workspace_archive::policy`): not the audit trail, the sync stream or export
//! jobs, and never credentials, capabilities, delivery queues and logs, or CSV
//! import history. Files under earlier exports, CSV import uploads, staged
//! uploads and workspace imports stay out too ([`is_exported_file`]).
//!
//! Password policy mirrors the whole-DB backup: a password seals the archive
//! (AES-256-GCM) and keeps the policy's secret columns; no password yields a
//! plaintext zip without them.
//!
//! People go as a fixed projection (`policy::ARCHIVE_USER_COLUMNS`) with their
//! primary email (`data/user_emails.json`), sealed or not: never a password,
//! MFA secret or sign-in identity of theirs. Their avatars, which live outside
//! the workspace's storage prefix, go under `avatars/`.

use std::collections::HashMap;
use std::io::{Cursor, Write};

use chrono::Utc;
use diesel::deserialize::QueryableByName;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Integer, Text};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::db::DbConnection;
use crate::services::backup::{seal_inner_zip, sha256_hex, table_exists_in_db, BackupError};
use crate::services::workspace_archive::{policy, rows};

/// Shared zip entry options (Deflated, 0644), matching the whole-DB backup.
fn zip_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644)
}

/// Manifest schema version for the workspace export. Distinct from the
/// crypto-envelope version `seal_inner_zip` stamps (that's the wire/crypto
/// format; this is the payload shape the Phase 2 import reads). Format 2 adds
/// people's primary emails, the users projection, avatars and the source.
pub const WORKSPACE_EXPORT_FORMAT_VERSION: u32 = 2;

/// Where a person's avatar sits in storage, and where it goes in an archive.
const AVATAR_STORAGE_PREFIX: &str = "users/avatars/";
const AVATAR_ARCHIVE_PREFIX: &str = "avatars/";

/// Storage prefixes an export leaves out: earlier exports, CSV import uploads
/// and grids, uploads not yet attached, and workspace archives waiting to
/// import.
const EXCLUDE_FILES_UNDER: &[&str] = &["exports/", "imports/", "temp/", "workspace-imports/"];

/// Whether the workspace file at `path` (workspace-relative) goes in an export.
pub fn is_exported_file(path: &str) -> bool {
    !EXCLUDE_FILES_UNDER
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

/// The export package manifest. Carries everything the Phase 2 import needs:
/// the workspace identity, the member user uuids referenced by tenant rows, and
/// per-table counts + integrity hashes.
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceExportManifest {
    pub workspace_export_format_version: u32,
    /// The Nosdesk version that made the export.
    pub nosdesk_version: String,
    /// `build.rs`'s `DefaultHasher` fingerprint of the migrations. Read only
    /// for an archive without `schema_sha256`: its hasher can change with the
    /// toolchain.
    pub schema_hash: String,
    /// The last migration applied when the export was taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_version: Option<String>,
    /// SHA-256 of every migration up to `migration_version` (`build.rs`), the
    /// schema marker import compares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_sha256: Option<String>,
    pub created_at: String,
    pub workspace_id: i32,
    pub workspace_slug: String,
    pub workspace_uuid: String,
    /// Member user uuids captured in `data/users.json`. Tenant rows reference
    /// users by uuid (never integer id), so the import resolves/upserts these
    /// against the target DB's global `users` rather than remapping them.
    pub member_user_uuids: Vec<String>,
    /// Per-table row count + sha256 of the JSON payload. Includes the scoped
    /// tenant tables plus `workspaces` (one row) and `users` (members).
    pub tables: HashMap<String, WorkspaceTableManifest>,
    /// Uploaded files bundled under `files/` (the workspace's `ws/{id}/` prefix).
    pub files: WorkspaceFilesManifest,
    /// People's avatars bundled under `avatars/`.
    #[serde(default)]
    pub avatars: WorkspaceFilesManifest,
    /// `self_hosted` or `hosted`: the kind of server that made the export.
    #[serde(default)]
    pub source_mode: String,
    /// The workspace's public host on that server, when it has one.
    #[serde(default)]
    pub source_origin: Option<String>,
    /// The workspace's custom domain there, when it had one.
    #[serde(default)]
    pub custom_domain: Option<String>,
    /// Sealed with a password.
    #[serde(default)]
    pub sealed: bool,
    /// Carries the policy's secret columns (only ever when sealed).
    #[serde(default)]
    pub secrets: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceTableManifest {
    pub count: i64,
    pub sha256: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WorkspaceFilesManifest {
    pub total_count: i64,
    pub total_size_bytes: i64,
}

#[derive(QueryableByName)]
struct RowText {
    #[diesel(sql_type = Text)]
    row_text: String,
}

/// Discover the workspace-scoped tables an archive carries: every public
/// ordinary/partitioned-parent base table with a live `workspace_id` column
/// whose policy class is carried (`policy::carried`). Introspected (not
/// hardcoded) so a future migration's tenant table is picked up automatically,
/// the same pattern the whole-DB backup uses.
///
/// Also the import's allowlist (`workspace_import::import_workspace`). Sharing
/// one definition is the point: "what may be exported" and "what may be
/// written back" are the same set by construction, so a new tenant table cannot
/// be exportable but not importable, and no table outside the set can be
/// written by an archive that names it.
pub(crate) fn discover_workspace_tables(
    conn: &mut DbConnection,
) -> Result<Vec<String>, BackupError> {
    Ok(all_workspace_tables(conn)?
        .into_iter()
        .filter(|t| policy::carried(t))
        .collect())
}

/// Every workspace-scoped table, carried or not.
pub(crate) fn all_workspace_tables(conn: &mut DbConnection) -> Result<Vec<String>, BackupError> {
    #[derive(QueryableByName)]
    struct TableName {
        #[diesel(sql_type = Text)]
        table_name: String,
    }

    let rows: Vec<TableName> = sql_query(
        "SELECT c.relname AS table_name \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_attribute a ON a.attrelid = c.oid \
         WHERE n.nspname = 'public' \
           AND c.relkind IN ('r','p') \
           AND c.relispartition = false \
           AND a.attname = 'workspace_id' \
           AND a.attnum > 0 \
           AND NOT a.attisdropped \
         ORDER BY c.relname",
    )
    .load(conn)
    .map_err(BackupError::DatabaseError)?;

    Ok(rows.into_iter().map(|r| r.table_name).collect())
}

/// Assemble a set of row-text JSON objects into a JSON-array text payload,
/// mirroring the whole-DB backup's on-disk shape.
fn assemble_array(rows: &[RowText]) -> String {
    let mut payload = String::from("[");
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            payload.push(',');
        }
        payload.push_str("\n  ");
        payload.push_str(&row.row_text);
    }
    if !rows.is_empty() {
        payload.push('\n');
    }
    payload.push(']');
    payload
}

/// Dump one table scoped to `workspace_id`. `where_sql` is a code-controlled
/// predicate that references the bound `$1` (= workspace id); `table_name` comes
/// from discovery and is re-validated against `information_schema` before it's
/// interpolated, so there is no injection surface.
fn dump_scoped(
    conn: &mut DbConnection,
    table_name: &str,
    where_sql: &str,
    workspace_id: i32,
    include_sensitive: bool,
) -> Result<(String, i64), BackupError> {
    let row_expr = rows::workspace_export_row_sql(table_name, include_sensitive);
    dump_rows(conn, table_name, &row_expr, where_sql, workspace_id)
}

/// Dump only `columns` of `table_name`'s rows matching `where_sql` (as for
/// [`dump_scoped`]).
fn dump_projected(
    conn: &mut DbConnection,
    table_name: &str,
    columns: &[&str],
    where_sql: &str,
    workspace_id: i32,
) -> Result<(String, i64), BackupError> {
    let fields = columns
        .iter()
        .map(|c| format!("'{c}', t.\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let row_expr = format!("jsonb_build_object({fields})::text");
    dump_rows(conn, table_name, &row_expr, where_sql, workspace_id)
}

fn dump_rows(
    conn: &mut DbConnection,
    table_name: &str,
    row_expr: &str,
    where_sql: &str,
    workspace_id: i32,
) -> Result<(String, i64), BackupError> {
    if !table_exists_in_db(conn, table_name)? {
        return Err(BackupError::CorruptedBackup(format!(
            "Refusing to export unknown table: {table_name}"
        )));
    }
    let query = format!("SELECT {row_expr} AS row_text FROM {table_name} t WHERE {where_sql}");
    let rows: Vec<RowText> = sql_query(&query)
        .bind::<Integer, _>(workspace_id)
        .load(conn)
        .map_err(BackupError::DatabaseError)?;
    let count = rows.len() as i64;
    Ok((assemble_array(&rows), count))
}

/// Workspace identity + the member uuids, fetched up front so the export fails
/// fast (clear error) on an unknown/deleted workspace before doing any work.
#[derive(Default)]
pub struct WorkspaceMeta {
    pub slug: String,
    pub uuid: String,
    pub member_user_uuids: Vec<String>,
    pub custom_domain: Option<String>,
    /// Storage keys (`users/avatars/...`) of the exported people's avatars,
    /// which the caller reads ([`read_avatars`]).
    pub avatar_keys: Vec<String>,
    /// Storage keys (`branding/...`) of branding images from before branding
    /// was workspace-scoped, outside the workspace's prefix, which the
    /// caller adds to its files ([`add_legacy_branding`]).
    pub legacy_branding_keys: Vec<String>,
    /// The rows carry the policy's secret columns.
    pub secrets: bool,
}

/// One table's dumped rows (JSON-text array) + row count. The row collection is
/// synchronous (DB); the caller assembles the archive after reading files.
pub struct WorkspaceRowDump {
    pub table: String,
    pub json: String,
    pub count: i64,
}

fn load_workspace_meta(
    conn: &mut DbConnection,
    workspace_id: i32,
) -> Result<WorkspaceMeta, BackupError> {
    #[derive(QueryableByName)]
    struct MetaRow {
        #[diesel(sql_type = Text)]
        slug: String,
        #[diesel(sql_type = Text)]
        uuid: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
        custom_domain: Option<String>,
    }
    let meta: MetaRow =
        sql_query("SELECT slug, uuid::text AS uuid, custom_domain FROM workspaces WHERE id = $1")
            .bind::<Integer, _>(workspace_id)
            .get_result(conn)
            .map_err(|e| match e {
                diesel::result::Error::NotFound => {
                    BackupError::CorruptedBackup(format!("workspace {workspace_id} not found"))
                }
                other => BackupError::DatabaseError(other),
            })?;

    #[derive(QueryableByName)]
    struct UuidRow {
        #[diesel(sql_type = Text)]
        user_uuid: String,
    }
    // members-any-status: an export must round-trip history, and historical rows
    // reference people who have since been removed; dropping them would fail the
    // enforced user FK on import.
    let members: Vec<UuidRow> = sql_query(
        "SELECT user_uuid::text AS user_uuid FROM workspace_members WHERE workspace_id = $1 \
         ORDER BY user_uuid",
    )
    .bind::<Integer, _>(workspace_id)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;

    Ok(WorkspaceMeta {
        slug: meta.slug,
        uuid: meta.uuid,
        member_user_uuids: members.into_iter().map(|r| r.user_uuid).collect(),
        custom_domain: meta.custom_domain,
        avatar_keys: Vec::new(),
        legacy_branding_keys: Vec::new(),
        secrets: false,
    })
}

/// Collect a workspace's row dumps: every scoped tenant table plus the
/// `workspaces` row and member `users`, as JSON-text arrays. Synchronous (DB
/// only) so it can run inside the BYPASSRLS transaction; files are read
/// separately (async, through the storage abstraction) and folded in by
/// [`assemble_workspace_archive`]. Must run BYPASSRLS so the scoped reads and the
/// global `workspaces`/`users` reads succeed regardless of session workspace pin.
pub fn collect_workspace_rows(
    conn: &mut DbConnection,
    workspace_id: i32,
    include_sensitive: bool,
) -> Result<(Vec<WorkspaceRowDump>, WorkspaceMeta), BackupError> {
    let mut meta = load_workspace_meta(conn, workspace_id)?;
    meta.secrets = include_sensitive;
    let mut dumps: Vec<WorkspaceRowDump> = Vec::new();

    for table in discover_workspace_tables(conn)? {
        let (json, count) = dump_scoped(
            conn,
            &table,
            "t.workspace_id = $1",
            workspace_id,
            include_sensitive,
        )?;
        dumps.push(WorkspaceRowDump { table, json, count });
    }

    let (ws_json, ws_count) = dump_scoped(
        conn,
        "workspaces",
        "t.id = $1",
        workspace_id,
        include_sensitive,
    )?;
    dumps.push(WorkspaceRowDump {
        table: "workspaces".to_string(),
        json: ws_json,
        count: ws_count,
    });

    // Users: the UNION of current members and every user uuid REFERENCED by an
    // exported row (requester/assignee/author/etc.). A row can reference a user
    // who was removed from membership or was never a member (external requester);
    // omitting them would fail the enforced user FK on import. Each referencing
    // column lives on a workspace-scoped table, so every subquery is scoped by $1.
    // members-any-status: as above, and the comment block above says so already.
    let mut union_parts =
        vec!["SELECT user_uuid FROM workspace_members WHERE workspace_id = $1".to_string()];
    for (tbl, col) in user_referencing_columns(conn)? {
        union_parts.push(format!(
            "SELECT \"{col}\" FROM \"{tbl}\" WHERE workspace_id = $1 AND \"{col}\" IS NOT NULL"
        ));
    }
    // And the people the references without a foreign key name (a uuid or a
    // uuid array each), so the import maps them rather than clearing them.
    for (tbl, col) in crate::services::workspace_archive::plan::USER_REFERENCES_WITHOUT_FK {
        union_parts.push(format!(
            "SELECT u FROM \"{tbl}\" t, unnest(ARRAY[t.\"{col}\"]) AS u \
             WHERE t.workspace_id = $1 AND u IS NOT NULL"
        ));
    }
    let people = union_parts.join(" UNION ");
    let users_where = format!("t.uuid IN ({people})");
    let (users_json, users_count) = dump_projected(
        conn,
        "users",
        policy::ARCHIVE_USER_COLUMNS,
        &users_where,
        workspace_id,
    )?;
    dumps.push(WorkspaceRowDump {
        table: "users".to_string(),
        json: users_json,
        count: users_count,
    });

    // Each person's primary email, so they can sign in where the workspace
    // lands. At most one per person (`user_emails_one_primary_per_user`).
    let (emails_json, emails_count) = dump_projected(
        conn,
        "user_emails",
        policy::ARCHIVE_USER_EMAIL_COLUMNS,
        &format!("t.is_primary AND t.user_uuid IN ({people})"),
        workspace_id,
    )?;
    dumps.push(WorkspaceRowDump {
        table: "user_emails".to_string(),
        json: emails_json,
        count: emails_count,
    });

    #[derive(QueryableByName)]
    struct Avatar {
        #[diesel(sql_type = Text)]
        avatar_url: String,
    }
    let avatars: Vec<Avatar> = sql_query(format!(
        "SELECT t.avatar_url FROM users t WHERE {users_where} AND t.avatar_url IS NOT NULL \
         ORDER BY t.uuid"
    ))
    .bind::<Integer, _>(workspace_id)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    meta.avatar_keys = avatars
        .iter()
        .filter_map(|a| avatar_storage_key(&a.avatar_url))
        .collect();
    // Two people can share one avatar file; it goes in once.
    meta.avatar_keys.sort_unstable();
    meta.avatar_keys.dedup();

    meta.legacy_branding_keys = legacy_branding_keys(conn, workspace_id)?;

    Ok((dumps, meta))
}

/// The storage keys of the workspace's branding images that predate
/// workspace-scoped branding (`/uploads/branding/logo_123.png`), stored
/// under the unscoped `branding/` folder every workspace shared.
fn legacy_branding_keys(
    conn: &mut DbConnection,
    workspace_id: i32,
) -> Result<Vec<String>, BackupError> {
    #[derive(QueryableByName)]
    struct Url {
        #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
        url: Option<String>,
    }
    let urls: Vec<Url> = sql_query(
        "SELECT unnest(ARRAY[logo_url::text, logo_light_url::text, favicon_url::text, \
                             email_logo->>'url', email_logo_light->>'url']) AS url \
         FROM site_settings WHERE workspace_id = $1",
    )
    .bind::<Integer, _>(workspace_id)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    let mut keys: Vec<String> = urls
        .iter()
        .filter_map(|u| u.url.as_deref())
        .filter_map(crate::handlers::branding::legacy_logical_path)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    Ok(keys)
}

/// Add the workspace's legacy branding images ([`WorkspaceMeta::legacy_branding_keys`])
/// to `files` at the workspace-relative path the import restores them to
/// (`branding/{name}`), unless `files` already holds that path. `storage` is
/// the unscoped store. An image whose file is gone is left out.
pub async fn add_legacy_branding(
    storage: &dyn crate::utils::storage::Storage,
    meta: &WorkspaceMeta,
    files: &mut Vec<(String, Vec<u8>)>,
) {
    for key in &meta.legacy_branding_keys {
        if files.iter().any(|(path, _)| path == key) {
            continue;
        }
        match storage.get_file(key).await {
            Ok(bytes) => files.push((key.clone(), bytes)),
            Err(e) => {
                tracing::warn!(error = ?e, "workspace export: branding image not readable, left out")
            }
        }
    }
}

/// The storage key of an uploaded avatar (`/uploads/users/avatars/x.webp`,
/// as the storage layer hands out, or the bare key), or `None` for an
/// external URL or anything that isn't one plain file under the avatars
/// prefix.
fn avatar_storage_key(avatar_url: &str) -> Option<String> {
    let key = avatar_url
        .strip_prefix("/uploads/")
        .or_else(|| avatar_url.strip_prefix("uploads/"))
        .unwrap_or(avatar_url)
        .trim_start_matches('/');
    let name = key.strip_prefix(AVATAR_STORAGE_PREFIX)?;
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && crate::utils::storage::is_safe_storage_path(key);
    plain.then(|| key.to_string())
}

/// Read the exported people's avatars from storage, as `(storage key,
/// bytes)`. An avatar whose file is gone is left out, and so is its
/// `avatar_url` ([`assemble_workspace_archive`]).
pub async fn read_avatars(
    storage: &dyn crate::utils::storage::Storage,
    meta: &WorkspaceMeta,
) -> Vec<(String, Vec<u8>)> {
    let mut avatars = Vec::with_capacity(meta.avatar_keys.len());
    for key in &meta.avatar_keys {
        match storage.get_file(key).await {
            Ok(bytes) => avatars.push((key.clone(), bytes)),
            Err(e) => {
                tracing::warn!(error = ?e, "workspace export: avatar not readable, left out")
            }
        }
    }
    avatars
}

/// The users dump with `avatar_url` cleared for anyone whose avatar file
/// isn't in `carried` (storage keys).
fn users_with_carried_avatars(
    json: &str,
    carried: &std::collections::HashSet<&str>,
) -> Result<String, BackupError> {
    let mut users: Vec<serde_json::Value> = serde_json::from_str(json)?;
    for user in &mut users {
        let Some(row) = user.as_object_mut() else {
            continue;
        };
        let keep = row
            .get("avatar_url")
            .and_then(serde_json::Value::as_str)
            .and_then(avatar_storage_key)
            .is_some_and(|key| carried.contains(key.as_str()));
        if !keep {
            row.insert("avatar_url".to_string(), serde_json::Value::Null);
        }
    }
    Ok(serde_json::to_string(&users)?)
}

/// The workspace's public host on this server: its canonical host on hosted,
/// else `FRONTEND_URL`'s host.
fn source_origin(meta: &WorkspaceMeta) -> Option<String> {
    crate::utils::tenant_origin::canonical_host_for(
        &meta.slug,
        meta.custom_domain.as_deref(),
        crate::utils::tenant_origin::tenant_domain().as_deref(),
    )
    .or_else(|| {
        let frontend = std::env::var("FRONTEND_URL").ok()?;
        url::Url::parse(frontend.trim())
            .ok()?
            .host_str()
            .map(str::to_string)
    })
}

/// The (table, column) pairs whose value is a `users.uuid` foreign key, on
/// workspace-scoped tables only (so the export can scope each by `workspace_id`).
/// Introspected from `pg_constraint` so a new user-referencing column is picked
/// up automatically. Used to collect every referenced user into the export.
pub(crate) fn user_referencing_columns(
    conn: &mut DbConnection,
) -> Result<Vec<(String, String)>, BackupError> {
    #[derive(QueryableByName)]
    struct Ref {
        #[diesel(sql_type = Text)]
        child_table: String,
        #[diesel(sql_type = Text)]
        child_column: String,
    }
    let rows: Vec<Ref> = sql_query(
        "SELECT con.conrelid::regclass::text AS child_table, \
                child_col.attname AS child_column \
         FROM pg_constraint con \
         JOIN pg_attribute child_col \
           ON child_col.attrelid = con.conrelid AND child_col.attnum = con.conkey[1] \
         JOIN pg_attribute parent_col \
           ON parent_col.attrelid = con.confrelid AND parent_col.attnum = con.confkey[1] \
         WHERE con.contype = 'f' \
           AND con.connamespace = 'public'::regnamespace \
           AND con.confrelid = 'public.users'::regclass \
           AND parent_col.attname = 'uuid' \
           AND array_length(con.conkey, 1) = 1 \
           AND EXISTS ( \
             SELECT 1 FROM pg_attribute wa \
             WHERE wa.attrelid = con.conrelid \
               AND wa.attname = 'workspace_id' \
               AND NOT wa.attisdropped \
           )",
    )
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    Ok(rows
        .into_iter()
        .map(|r| (r.child_table, r.child_column))
        .collect())
}

/// Assemble the final archive from the collected row dumps and the workspace's
/// files (`logical path -> bytes`, read through the storage abstraction so it
/// works for local and S3 alike). Files are stored under `files/{logical}`
/// (workspace-relative). Seals with the password when present.
pub fn assemble_workspace_archive(
    dumps: &[WorkspaceRowDump],
    meta: &WorkspaceMeta,
    workspace_id: i32,
    files: &[(String, Vec<u8>)],
    avatars: &[(String, Vec<u8>)],
    password: Option<&str>,
) -> Result<Vec<u8>, BackupError> {
    let cursor: Cursor<Vec<u8>> = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);

    let carried: std::collections::HashSet<&str> = avatars
        .iter()
        .filter(|(key, _)| avatar_storage_key(key).is_some())
        .map(|(key, _)| key.as_str())
        .collect();

    let mut table_manifests: HashMap<String, WorkspaceTableManifest> = HashMap::new();
    for dump in dumps {
        let users_json;
        let json = if dump.table == "users" {
            users_json = users_with_carried_avatars(&dump.json, &carried)?;
            &users_json
        } else {
            &dump.json
        };
        zip.start_file(format!("data/{}.json", dump.table), zip_options())?;
        zip.write_all(json.as_bytes())?;
        table_manifests.insert(
            dump.table.clone(),
            WorkspaceTableManifest {
                count: dump.count,
                sha256: sha256_hex(json.as_bytes()),
            },
        );
    }

    let mut files_manifest = WorkspaceFilesManifest::default();
    for (logical, bytes) in files {
        zip.start_file(format!("files/{logical}"), zip_options())?;
        zip.write_all(bytes)?;
        files_manifest.total_count += 1;
        files_manifest.total_size_bytes += bytes.len() as i64;
    }

    let mut avatars_manifest = WorkspaceFilesManifest::default();
    let mut written = std::collections::HashSet::new();
    for (key, bytes) in avatars {
        let Some(name) = carried
            .contains(key.as_str())
            .then(|| key.strip_prefix(AVATAR_STORAGE_PREFIX))
            .flatten()
        else {
            continue;
        };
        if !written.insert(name) {
            continue;
        }
        zip.start_file(format!("{AVATAR_ARCHIVE_PREFIX}{name}"), zip_options())?;
        zip.write_all(bytes)?;
        avatars_manifest.total_count += 1;
        avatars_manifest.total_size_bytes += bytes.len() as i64;
    }

    let manifest = WorkspaceExportManifest {
        workspace_export_format_version: WORKSPACE_EXPORT_FORMAT_VERSION,
        nosdesk_version: env!("CARGO_PKG_VERSION").to_string(),
        schema_hash: env!("NOSDESK_SCHEMA_HASH").to_string(),
        migration_version: Some(
            crate::services::backup_upgrade::current_schema()
                .version
                .to_string(),
        ),
        schema_sha256: Some(
            crate::services::backup_upgrade::current_schema()
                .sha256
                .to_string(),
        ),
        created_at: Utc::now().to_rfc3339(),
        workspace_id,
        workspace_slug: meta.slug.clone(),
        workspace_uuid: meta.uuid.clone(),
        member_user_uuids: meta.member_user_uuids.clone(),
        tables: table_manifests,
        files: files_manifest,
        avatars: avatars_manifest,
        source_mode: if crate::middleware::workspace_context::is_hosted() {
            "hosted"
        } else {
            "self_hosted"
        }
        .to_string(),
        source_origin: source_origin(meta),
        custom_domain: meta.custom_domain.clone(),
        sealed: password.is_some(),
        secrets: meta.secrets && password.is_some(),
    };
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    zip.start_file("manifest.json", zip_options())?;
    zip.write_all(manifest_json.as_bytes())?;

    let inner = zip.finish()?.into_inner();
    match password {
        Some(pw) => seal_inner_zip(&inner, pw),
        None => Ok(inner),
    }
}

/// Why [`build_archive`] made no archive.
#[derive(Debug)]
pub enum BuildError {
    /// Past the caller's byte cap.
    TooLarge,
    Failed(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::TooLarge => f.write_str("the workspace is larger than the export cap"),
            BuildError::Failed(e) => f.write_str(e),
        }
    }
}

/// Build workspace `workspace_id`'s whole archive: its rows (read elevated),
/// its files, its people's avatars and its legacy branding, sealed with
/// `password` when given. `include_sensitive` keeps the policy's secret
/// columns, which only a sealed archive may carry. With `cap`, the rows and
/// files are summed against it as they are read, and a larger workspace is
/// refused before it is held whole. `storage` is the unscoped store.
pub async fn build_archive(
    pool: &crate::db::Pool,
    storage: std::sync::Arc<dyn crate::utils::storage::Storage>,
    workspace_id: i32,
    password: Option<&str>,
    include_sensitive: bool,
    cap: Option<usize>,
) -> Result<Vec<u8>, BuildError> {
    let include_sensitive = include_sensitive && password.is_some();
    let over = |total: usize| cap.is_some_and(|cap| total > cap);

    // 1. Row dumps (sync, BYPASSRLS).
    let pool = pool.clone();
    let (dumps, meta) = tokio::task::spawn_blocking(move || {
        let mut conn = pool.get().map_err(|e| format!("pool: {e}"))?;
        let actor = crate::sync::actor::ActorContext::system("background:workspace_export")
            .with_workspace(workspace_id);
        // elevated-in-workspace: the export reads one workspace's rows, filtered by its id, and its people's global users rows.
        crate::sync::session::with_actor_bypass_context(&mut conn, &actor, |conn| {
            collect_workspace_rows(conn, workspace_id, include_sensitive)
                .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
        })
        .map_err(|e| format!("collect rows: {e}"))
    })
    .await
    .map_err(|e| BuildError::Failed(e.to_string()))?
    .map_err(BuildError::Failed)?;
    let mut total: usize = dumps.iter().map(|d| d.json.len()).sum();
    if over(total) {
        return Err(BuildError::TooLarge);
    }

    // 2. Files, through the storage abstraction (local or S3).
    let scoped = crate::utils::storage::WorkspaceScopedStorage::arc(storage.clone(), workspace_id);
    let paths = scoped
        .list_prefix("")
        .await
        .map_err(|e| BuildError::Failed(format!("list files: {e:?}")))?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::with_capacity(paths.len());
    for p in paths.into_iter().filter(|p| is_exported_file(p)) {
        let bytes = scoped
            .get_file(&p)
            .await
            .map_err(|e| BuildError::Failed(format!("read file {p}: {e:?}")))?;
        total += bytes.len();
        if over(total) {
            return Err(BuildError::TooLarge);
        }
        files.push((p, bytes));
    }

    // 3. People's avatars, and branding from before it was workspace-scoped,
    //    which live outside the workspace's prefix.
    let carried = files.len();
    add_legacy_branding(storage.as_ref(), &meta, &mut files).await;
    total += files[carried..]
        .iter()
        .map(|(_, bytes)| bytes.len())
        .sum::<usize>();
    let avatars = read_avatars(storage.as_ref(), &meta).await;
    total += avatars.iter().map(|(_, bytes)| bytes.len()).sum::<usize>();
    if over(total) {
        return Err(BuildError::TooLarge);
    }

    // 4. Assemble and seal (sync CPU).
    let password = password.map(str::to_string);
    tokio::task::spawn_blocking(move || {
        assemble_workspace_archive(
            &dumps,
            &meta,
            workspace_id,
            &files,
            &avatars,
            password.as_deref(),
        )
        .map_err(|e| BuildError::Failed(format!("assemble archive: {e}")))
    })
    .await
    .map_err(|e| BuildError::Failed(e.to_string()))?
}

/// Convenience: export a workspace's ROWS ONLY (no files) in one synchronous
/// call. The file-inclusive path lives in the handler, which reads files through
/// the async storage abstraction. Used by tests and as the archive source for
/// the import round-trip. Must run BYPASSRLS.
pub fn export_workspace(
    conn: &mut DbConnection,
    workspace_id: i32,
    password: Option<&str>,
) -> Result<Vec<u8>, BackupError> {
    let include_sensitive = password.is_some();
    let (dumps, meta) = collect_workspace_rows(conn, workspace_id, include_sensitive)?;
    assemble_workspace_archive(&dumps, &meta, workspace_id, &[], &[], password)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::actor::ActorContext;
    use crate::sync::session::with_actor_bypass_context;
    use crate::test_helpers::setup_test_pool;
    use std::io::Read;

    /// A qb-error wrapper so the export (BackupError) fits the bypass closure's
    /// `Result<_, diesel::Error>` signature, mirroring the backup handler.
    fn as_diesel<T>(r: Result<T, BackupError>) -> Result<T, diesel::result::Error> {
        r.map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    }

    #[test]
    fn exports_a_scoped_workspace_archive() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("test pool connection");
        let actor = ActorContext::system("test:workspace_export");

        // A real workspace id (the bootstrap seed provides one).
        #[derive(QueryableByName)]
        struct IdRow {
            #[diesel(sql_type = Integer)]
            id: i32,
        }
        let ws_id: i32 = with_actor_bypass_context(&mut conn, &actor, |c| {
            let r: IdRow =
                sql_query("SELECT id FROM workspaces ORDER BY id LIMIT 1").get_result(c)?;
            Ok::<_, diesel::result::Error>(r.id)
        })
        .expect("a workspace exists in the test DB");

        // Plaintext export: a bare zip we can read directly. Must run BYPASSRLS,
        // else RLS (no workspace pin) filters every tenant table to zero rows.
        let bytes = with_actor_bypass_context(&mut conn, &actor, |c| {
            as_diesel(export_workspace(c, ws_id, None))
        })
        .expect("export succeeds");
        assert_eq!(
            &bytes[0..4],
            b"PK\x03\x04",
            "plaintext export is a bare zip"
        );

        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("valid zip archive");
        let manifest: WorkspaceExportManifest = {
            let mut f = archive.by_name("manifest.json").expect("manifest present");
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            serde_json::from_str(&s).expect("manifest parses")
        };

        assert_eq!(manifest.workspace_id, ws_id);
        assert!(!manifest.workspace_slug.is_empty(), "slug captured");
        assert_eq!(
            manifest.workspace_export_format_version,
            WORKSPACE_EXPORT_FORMAT_VERSION
        );
        // The workspace row is exactly one; users are dumped; scoped tenant
        // tables are present (even empty ones are discovered + listed).
        assert_eq!(
            manifest.tables.get("workspaces").map(|t| t.count),
            Some(1),
            "exactly one workspace row"
        );
        assert!(manifest.tables.contains_key("users"), "member users dumped");
        assert!(
            manifest.tables.contains_key("workflow_states"),
            "scoped tenant tables discovered"
        );
        // The excluded partitioned tables must never appear.
        assert!(
            !manifest.tables.contains_key("audit_log"),
            "audit_log excluded"
        );
        assert!(
            !manifest.tables.contains_key("sync_actions"),
            "sync_actions excluded"
        );
        assert!(
            archive.by_name("data/workspaces.json").is_ok(),
            "per-table data entry written"
        );
        // Files section is present. The test DB has no uploaded files on disk
        // (no `{uploads}/ws/{id}/`), so the walk's empty-directory path yields an
        // empty manifest rather than erroring.
        assert_eq!(
            manifest.files.total_count, 0,
            "no uploaded files for the test workspace"
        );

        // A password seals the archive with the NODB envelope.
        let sealed = with_actor_bypass_context(&mut conn, &actor, |c| {
            as_diesel(export_workspace(c, ws_id, Some("test-password")))
        })
        .expect("sealed export succeeds");
        assert_eq!(&sealed[0..4], b"NODB", "password export is sealed");

        // Format 2 records where it came from, and its schema point.
        let current = crate::services::backup_upgrade::current_schema();
        assert_eq!(manifest.workspace_export_format_version, 2);
        assert_eq!(manifest.migration_version.as_deref(), Some(current.version));
        assert_eq!(manifest.schema_sha256.as_deref(), Some(current.sha256));
        assert_eq!(manifest.nosdesk_version, env!("CARGO_PKG_VERSION"));
        assert!(matches!(
            manifest.source_mode.as_str(),
            "self_hosted" | "hosted"
        ));
        assert!(!manifest.sealed && !manifest.secrets);
        assert!(manifest.tables.contains_key("user_emails"));
        let sealed_manifest: WorkspaceExportManifest = {
            let inner = crate::services::backup::unseal_inner_zip(&sealed, "test-password")
                .expect("unseal");
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(inner)).expect("zip");
            let mut f = archive.by_name("manifest.json").expect("manifest");
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            serde_json::from_str(&s).expect("manifest parses")
        };
        assert!(sealed_manifest.sealed && sealed_manifest.secrets);
    }

    #[test]
    fn only_plain_uploaded_avatars_are_carried() {
        assert_eq!(
            avatar_storage_key("/uploads/users/avatars/0190_avatar.webp").as_deref(),
            Some("users/avatars/0190_avatar.webp")
        );
        assert_eq!(
            avatar_storage_key("users/avatars/a.png").as_deref(),
            Some("users/avatars/a.png")
        );
        for other in [
            "https://example.com/a.png",
            "/uploads/users/banners/a.png",
            "/uploads/users/avatars/",
            "/uploads/users/avatars/../secrets.txt",
            "/uploads/users/avatars/x/y.png",
        ] {
            assert_eq!(avatar_storage_key(other), None, "{other}");
        }
    }

    /// A person whose avatar file isn't carried arrives without one.
    #[test]
    fn avatar_urls_stay_only_with_their_files() {
        let users = r#"[
            {"uuid": "a", "avatar_url": "/uploads/users/avatars/a.webp"},
            {"uuid": "b", "avatar_url": "/uploads/users/avatars/b.webp"},
            {"uuid": "c", "avatar_url": "https://example.com/c.png"},
            {"uuid": "d", "avatar_url": null}
        ]"#;
        let dumps = vec![WorkspaceRowDump {
            table: "users".to_string(),
            json: users.to_string(),
            count: 4,
        }];
        let avatars = vec![("users/avatars/a.webp".to_string(), b"webp".to_vec())];
        let bytes =
            assemble_workspace_archive(&dumps, &WorkspaceMeta::default(), 7, &[], &avatars, None)
                .expect("assemble");
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("valid zip");
        let mut read = |name: &str| {
            let mut f = archive.by_name(name).expect(name);
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            s
        };
        let users: serde_json::Value = serde_json::from_str(&read("data/users.json")).unwrap();
        let urls: Vec<_> = users
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u["avatar_url"].as_str())
            .collect();
        assert_eq!(
            urls,
            vec![Some("/uploads/users/avatars/a.webp"), None, None, None]
        );
        assert_eq!(read("avatars/a.webp"), "webp");
        let manifest: WorkspaceExportManifest =
            serde_json::from_str(&read("manifest.json")).unwrap();
        assert_eq!(manifest.avatars.total_count, 1);
        assert_eq!(
            manifest.tables["users"].sha256,
            sha256_hex(read("data/users.json").as_bytes()),
            "the checksum covers the rewritten rows"
        );
    }

    #[test]
    fn assembles_files_under_workspace_relative_paths() {
        use std::io::Read;

        let dumps = vec![WorkspaceRowDump {
            table: "workspaces".to_string(),
            json: "[]".to_string(),
            count: 0,
        }];
        let meta = WorkspaceMeta {
            slug: "acme".to_string(),
            uuid: "00000000-0000-0000-0000-000000000001".to_string(),
            ..Default::default()
        };
        let files = vec![
            ("tickets/5/foo.png".to_string(), b"hello".to_vec()),
            ("assets/bar.pdf".to_string(), b"xyz".to_vec()),
        ];

        let bytes =
            assemble_workspace_archive(&dumps, &meta, 7, &files, &[], None).expect("assemble");
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("valid zip");

        // Files land under `files/{logical}` (workspace-relative, no ws/{id}/).
        assert!(archive.by_name("files/tickets/5/foo.png").is_ok());
        assert!(archive.by_name("files/assets/bar.pdf").is_ok());

        let manifest: WorkspaceExportManifest = {
            let mut f = archive.by_name("manifest.json").unwrap();
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            serde_json::from_str(&s).unwrap()
        };
        assert_eq!(manifest.files.total_count, 2);
        assert_eq!(manifest.files.total_size_bytes, 8, "5 + 3 bytes");
    }

    #[test]
    fn exports_leave_out_earlier_exports_imports_and_staged_uploads() {
        for kept in [
            "tickets/5/foo.png",
            "assets/9/media/photo.jpg",
            "documentation/1/image.png",
            "importsx/keep.txt",
        ] {
            assert!(is_exported_file(kept), "{kept} is exported");
        }
        for left_out in [
            "exports/0190.nosdesk",
            "imports/0190/upload.csv",
            "imports/0190/grid.1.json",
            "temp/0190_photo.png",
            "workspace-imports/0190.nosdesk",
        ] {
            assert!(!is_exported_file(left_out), "{left_out} is left out");
        }
    }
}
