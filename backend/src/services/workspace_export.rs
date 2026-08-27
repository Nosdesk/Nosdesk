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
//! Excluded: the partitioned `audit_log` + `sync_actions` (audit trail + sync
//! stream — high-churn, not core tenant data, and partitioned parents that
//! complicate a scoped dump).
//!
//! Password policy mirrors the whole-DB backup: a password seals the archive
//! (AES-256-GCM) and keeps sensitive auth fields; no password yields a plaintext
//! zip with `SENSITIVE_FIELDS` stripped.

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
use crate::services::backup::{
    seal_inner_zip, sha256_hex, table_exists_in_db, BackupError, SENSITIVE_FIELDS,
};

/// Shared zip entry options (Deflated, 0644), matching the whole-DB backup.
fn zip_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644)
}

/// Manifest schema version for the workspace export. Distinct from the
/// crypto-envelope version `seal_inner_zip` stamps (that's the wire/crypto
/// format; this is the payload shape the Phase 2 import reads).
const WORKSPACE_EXPORT_FORMAT_VERSION: u32 = 1;

/// Workspace-scoped tables excluded from the export: the two partitioned,
/// high-churn tables. They carry `workspace_id` but are the audit trail and the
/// sync outbox, not core tenant data, and their partitioned shape complicates a
/// scoped dump (matching the whole-DB backup's parent-only handling).
// `workspace_export_jobs` carries a workspace_id but is operational metadata (job
// status + storage keys), not tenant content, so it is excluded like audit_log.
const EXCLUDE_FROM_WORKSPACE_EXPORT: &[&str] =
    &["audit_log", "sync_actions", "workspace_export_jobs"];

/// The export package manifest. Carries everything the Phase 2 import needs:
/// the workspace identity, the member user uuids referenced by tenant rows, and
/// per-table counts + integrity hashes.
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceExportManifest {
    pub workspace_export_format_version: u32,
    pub nosdesk_version: String,
    pub schema_hash: String,
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

/// Discover the workspace-scoped tables: every public ordinary/partitioned-parent
/// base table carrying a live `workspace_id` column, minus the excluded ones.
/// Introspected (not hardcoded) so a future migration's tenant table is picked
/// up automatically, the same pattern the whole-DB backup uses.
///
/// Also the import's allowlist (`workspace_import::import_workspace`). Sharing
/// one definition is the point: "what may be exported" and "what may be
/// written back" are the same set by construction, so a new tenant table cannot
/// be exportable but not importable, and no table outside the set can be
/// written by an archive that names it.
pub(crate) fn discover_workspace_tables(
    conn: &mut DbConnection,
) -> Result<Vec<String>, BackupError> {
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

    Ok(rows
        .into_iter()
        .map(|r| r.table_name)
        .filter(|t| !EXCLUDE_FROM_WORKSPACE_EXPORT.contains(&t.as_str()))
        .collect())
}

/// The per-row `SELECT` projection: `row_to_json(t)` kept as text end-to-end so
/// `NUMERIC` precision survives, with `SENSITIVE_FIELDS` stripped inside Postgres
/// (`::jsonb - 'key'`) for plaintext exports. Identical policy to
/// `backup::export_table_data`.
fn row_projection(table_name: &str, include_sensitive: bool) -> String {
    let strip_fields: &[&str] = if include_sensitive {
        &[]
    } else {
        SENSITIVE_FIELDS
            .iter()
            .find(|(t, _)| *t == table_name)
            .map(|(_, fields)| *fields)
            .unwrap_or(&[])
    };

    if strip_fields.is_empty() {
        "row_to_json(t)::text".to_string()
    } else {
        let mut expr = "row_to_json(t)::jsonb".to_string();
        for f in strip_fields {
            let escaped = f.replace('\'', "''");
            expr.push_str(&format!(" - '{escaped}'"));
        }
        format!("({expr})::text")
    }
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
    if !table_exists_in_db(conn, table_name)? {
        return Err(BackupError::CorruptedBackup(format!(
            "Refusing to export unknown table: {table_name}"
        )));
    }
    let row_expr = row_projection(table_name, include_sensitive);
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
pub struct WorkspaceMeta {
    pub slug: String,
    pub uuid: String,
    pub member_user_uuids: Vec<String>,
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
    }
    let meta: MetaRow = sql_query("SELECT slug, uuid::text AS uuid FROM workspaces WHERE id = $1")
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
    let meta = load_workspace_meta(conn, workspace_id)?;
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
    let mut union_parts =
        vec!["SELECT user_uuid FROM workspace_members WHERE workspace_id = $1".to_string()];
    for (tbl, col) in user_referencing_columns(conn)? {
        union_parts.push(format!(
            "SELECT \"{col}\" FROM \"{tbl}\" WHERE workspace_id = $1 AND \"{col}\" IS NOT NULL"
        ));
    }
    let users_where = format!("t.uuid IN ({})", union_parts.join(" UNION "));
    let (users_json, users_count) =
        dump_scoped(conn, "users", &users_where, workspace_id, include_sensitive)?;
    dumps.push(WorkspaceRowDump {
        table: "users".to_string(),
        json: users_json,
        count: users_count,
    });

    Ok((dumps, meta))
}

/// The (table, column) pairs whose value is a `users.uuid` foreign key, on
/// workspace-scoped tables only (so the export can scope each by `workspace_id`).
/// Introspected from `pg_constraint` so a new user-referencing column is picked
/// up automatically. Used to collect every referenced user into the export.
fn user_referencing_columns(conn: &mut DbConnection) -> Result<Vec<(String, String)>, BackupError> {
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
    password: Option<&str>,
) -> Result<Vec<u8>, BackupError> {
    let cursor: Cursor<Vec<u8>> = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);

    let mut table_manifests: HashMap<String, WorkspaceTableManifest> = HashMap::new();
    for dump in dumps {
        zip.start_file(format!("data/{}.json", dump.table), zip_options())?;
        zip.write_all(dump.json.as_bytes())?;
        table_manifests.insert(
            dump.table.clone(),
            WorkspaceTableManifest {
                count: dump.count,
                sha256: sha256_hex(dump.json.as_bytes()),
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

    let manifest = WorkspaceExportManifest {
        workspace_export_format_version: WORKSPACE_EXPORT_FORMAT_VERSION,
        nosdesk_version: env!("CARGO_PKG_VERSION").to_string(),
        schema_hash: env!("NOSDESK_SCHEMA_HASH").to_string(),
        created_at: Utc::now().to_rfc3339(),
        workspace_id,
        workspace_slug: meta.slug.clone(),
        workspace_uuid: meta.uuid.clone(),
        member_user_uuids: meta.member_user_uuids.clone(),
        tables: table_manifests,
        files: files_manifest,
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
    assemble_workspace_archive(&dumps, &meta, workspace_id, &[], password)
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
            member_user_uuids: vec![],
        };
        let files = vec![
            ("tickets/5/foo.png".to_string(), b"hello".to_vec()),
            ("assets/bar.pdf".to_string(), b"xyz".to_vec()),
        ];

        let bytes = assemble_workspace_archive(&dumps, &meta, 7, &files, None).expect("assemble");
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
}
