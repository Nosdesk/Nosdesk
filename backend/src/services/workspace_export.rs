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
//! Uploaded files are bundled from the workspace's local `ws/{id}/` prefix under
//! `files/`, mirroring the whole-DB backup's local-filesystem walk (S3-backed
//! storage isn't walked, a pre-existing limitation of that path too).
//!
//! Excluded: the partitioned `audit_log` + `sync_actions` (audit trail + sync
//! stream — high-churn, not core tenant data, and partitioned parents that
//! complicate a scoped dump).
//!
//! Password policy mirrors the whole-DB backup: a password seals the archive
//! (AES-256-GCM) and keeps sensitive auth fields; no password yields a plaintext
//! zip with `SENSITIVE_FIELDS` stripped.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read, Write};

use chrono::Utc;
use diesel::deserialize::QueryableByName;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Integer, Text};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use zip::write::FileOptions;
use zip::ZipWriter;

use crate::db::DbConnection;
use crate::services::backup::{
    get_uploads_dir, seal_inner_zip, sha256_hex, table_exists_in_db, BackupError, SENSITIVE_FIELDS,
};

/// Shared zip entry options (Deflated, 0644), matching the whole-DB backup.
fn zip_options() -> FileOptions {
    FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644)
}

/// Walk the workspace's local uploaded files (`{uploads}/ws/{id}/`) into the zip
/// under `files/{path-relative-to-uploads}`, returning the count + total bytes.
/// A missing directory (workspace has no files) yields an empty manifest.
fn bundle_workspace_files(
    zip: &mut ZipWriter<Cursor<Vec<u8>>>,
    uploads_dir: &std::path::Path,
    workspace_id: i32,
) -> Result<WorkspaceFilesManifest, BackupError> {
    let ws_dir = uploads_dir.join("ws").join(workspace_id.to_string());
    let mut manifest = WorkspaceFilesManifest::default();
    if !ws_dir.exists() {
        return Ok(manifest);
    }
    for entry in WalkDir::new(&ws_dir).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let file_path = entry.path();
        let relative = file_path
            .strip_prefix(uploads_dir)
            .map_err(|e| BackupError::IoError(std::io::Error::other(e.to_string())))?;
        zip.start_file(format!("files/{}", relative.display()), zip_options())?;
        let mut f = File::open(file_path)?;
        let mut buf = Vec::new();
        let size = f.read_to_end(&mut buf)?;
        zip.write_all(&buf)?;
        manifest.total_count += 1;
        manifest.total_size_bytes += size as i64;
    }
    Ok(manifest)
}

/// Manifest schema version for the workspace export. Distinct from the
/// crypto-envelope version `seal_inner_zip` stamps (that's the wire/crypto
/// format; this is the payload shape the Phase 2 import reads).
const WORKSPACE_EXPORT_FORMAT_VERSION: u32 = 1;

/// Workspace-scoped tables excluded from the export: the two partitioned,
/// high-churn tables. They carry `workspace_id` but are the audit trail and the
/// sync outbox, not core tenant data, and their partitioned shape complicates a
/// scoped dump (matching the whole-DB backup's parent-only handling).
const EXCLUDE_FROM_WORKSPACE_EXPORT: &[&str] = &["audit_log", "sync_actions"];

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
fn discover_workspace_tables(conn: &mut DbConnection) -> Result<Vec<String>, BackupError> {
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
struct WorkspaceMeta {
    slug: String,
    uuid: String,
    member_user_uuids: Vec<String>,
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

/// Build the plaintext inner zip: `data/{table}.json` for each scoped tenant
/// table plus `workspaces` and `users`, and `manifest.json`. The caller seals it
/// when a password is supplied.
fn build_workspace_inner_zip(
    conn: &mut DbConnection,
    workspace_id: i32,
    meta: &WorkspaceMeta,
    include_sensitive: bool,
) -> Result<Vec<u8>, BackupError> {
    use std::io::Cursor;

    let cursor: Cursor<Vec<u8>> = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(cursor);
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut table_manifests: HashMap<String, WorkspaceTableManifest> = HashMap::new();

    let mut write_table = |zip: &mut ZipWriter<Cursor<Vec<u8>>>,
                           name: &str,
                           json: &str,
                           count: i64|
     -> Result<(), BackupError> {
        zip.start_file(format!("data/{name}.json"), options)?;
        zip.write_all(json.as_bytes())?;
        table_manifests.insert(
            name.to_string(),
            WorkspaceTableManifest {
                count,
                sha256: sha256_hex(json.as_bytes()),
            },
        );
        Ok(())
    };

    // Scoped tenant tables.
    for table in discover_workspace_tables(conn)? {
        let (json, count) = dump_scoped(
            conn,
            &table,
            "t.workspace_id = $1",
            workspace_id,
            include_sensitive,
        )?;
        write_table(&mut zip, &table, &json, count)?;
    }

    // The workspace row itself (global table).
    let (ws_json, ws_count) = dump_scoped(
        conn,
        "workspaces",
        "t.id = $1",
        workspace_id,
        include_sensitive,
    )?;
    write_table(&mut zip, "workspaces", &ws_json, ws_count)?;

    // Member users (global, referenced by uuid from tenant rows).
    let (users_json, users_count) = dump_scoped(
        conn,
        "users",
        "t.uuid IN (SELECT user_uuid FROM workspace_members WHERE workspace_id = $1)",
        workspace_id,
        include_sensitive,
    )?;
    write_table(&mut zip, "users", &users_json, users_count)?;

    // Uploaded files: everything physically under the workspace's `ws/{id}/`
    // prefix in the local uploads dir, archived under `files/{path-from-uploads}`
    // so the import can restore it verbatim. This mirrors the whole-DB backup's
    // local-filesystem walk; S3-backed storage is not walked here (a pre-existing
    // limitation of the backup path too, tracked separately).
    let files = bundle_workspace_files(&mut zip, &get_uploads_dir(), workspace_id)?;

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
        files,
    };
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    zip.start_file("manifest.json", options)?;
    zip.write_all(manifest_json.as_bytes())?;

    let finished = zip.finish()?;
    Ok(finished.into_inner())
}

/// Export one workspace to an encrypted (password) or plaintext (no password)
/// archive, returned as bytes. Must be called inside `with_actor_bypass_context`
/// (BYPASSRLS) so the scoped reads and the global `workspaces`/`users` reads
/// succeed regardless of session workspace pin.
pub fn export_workspace(
    conn: &mut DbConnection,
    workspace_id: i32,
    password: Option<&str>,
) -> Result<Vec<u8>, BackupError> {
    let include_sensitive = password.is_some();
    let meta = load_workspace_meta(conn, workspace_id)?;
    let inner = build_workspace_inner_zip(conn, workspace_id, &meta, include_sensitive)?;
    match password {
        Some(pw) => seal_inner_zip(&inner, pw),
        None => Ok(inner),
    }
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
    fn bundles_only_the_workspaces_own_files() {
        use std::fs;

        let root =
            std::env::temp_dir().join(format!("nosdesk-wsexport-files-{}", std::process::id()));
        let ws_id = 7;
        let ws_dir = root.join("ws").join(ws_id.to_string()).join("sub");
        fs::create_dir_all(&ws_dir).unwrap();
        fs::write(ws_dir.join("foo.txt"), b"hello").unwrap();
        // A different workspace's file must not leak into this export.
        let other = root.join("ws").join("999");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("bar.txt"), b"nope").unwrap();

        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let manifest = bundle_workspace_files(&mut zip, &root, ws_id).expect("bundle");
        let bytes = zip.finish().unwrap().into_inner();

        assert_eq!(manifest.total_count, 1, "exactly one file for workspace 7");
        assert_eq!(manifest.total_size_bytes, 5, "byte count of foo.txt");

        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(
            archive.by_name("files/ws/7/sub/foo.txt").is_ok(),
            "workspace file bundled at its uploads-relative path"
        );
        assert!(
            archive.by_name("files/ws/999/bar.txt").is_err(),
            "another workspace's file is excluded"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
