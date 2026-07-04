//! Workspace-scoped import — Phase 2 of the tenant export/import primitive.
//!
//! Reads a Phase 1 export archive (see `workspace_export`) and reconstructs the
//! workspace in THIS database as a NEW workspace with a fresh integer id,
//! remapping every integer primary key and foreign key so it can't collide with
//! the pooled cell's other tenants. Users are matched/upserted by `uuid`
//! (global, never remapped). The whole import runs in one transaction and rolls
//! back on any error.
//!
//! Execution model: the import runs on a BYPASSRLS connection (the caller wraps
//! it in `with_actor_bypass_context` / `PlatformConn`) and sets
//! `nosdesk.in_audit_read = 'true'` for the transaction, which short-circuits
//! both audit-capture triggers. Sync events are never generated (the import
//! writes raw, not through `sync::emit`), and FK integrity comes from the
//! topological insert order — so no `session_replication_role` / superuser is
//! required, and it works on the pooled hosted connection. Integer primary keys
//! are drawn fresh from each table's sequence and old ids are rewritten across
//! the FK graph; users are matched/upserted by uuid (never remapped).

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};

use diesel::deserialize::QueryableByName;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};
use serde_json::{json, Value};

use crate::services::backup::{table_exists_in_db, unseal_inner_zip, BackupError};
use crate::services::workspace_export::WorkspaceExportManifest;

/// Envelope magic for a sealed archive (mirrors `backup::ENCRYPTED_MAGIC`); a
/// plaintext archive is a bare zip (`PK\x03\x04`).
const ENCRYPTED_MAGIC: &[u8; 4] = b"NODB";

/// The parsed, verified contents of an export archive, ready to import.
pub struct ArchiveContents {
    pub manifest: WorkspaceExportManifest,
    /// `data/{table}.json` parsed into rows, keyed by table name.
    pub tables: HashMap<String, Vec<Value>>,
    /// `files/{path}` entries: the archive-relative path and the bytes.
    pub files: Vec<(String, Vec<u8>)>,
}

/// A single-column integer foreign-key edge: `child_table.child_column`
/// references `parent_table.id`. Drives both the remap (rewrite the child column
/// via the parent's id map) and the topological insert order.
#[derive(Debug, Clone)]
pub struct FkEdge {
    pub child_table: String,
    pub child_column: String,
    pub parent_table: String,
}

/// Read + verify an export archive from bytes. Unseals when the `NODB` envelope
/// is present (password required), then unzips and parses the manifest, the
/// per-table row arrays, and the file blobs. Refuses on a schema-hash or
/// format-version mismatch — a different schema means a different FK graph /
/// column set, so a remap would be unsafe.
pub fn read_archive(
    archive: &[u8],
    password: Option<&str>,
) -> Result<ArchiveContents, BackupError> {
    let inner: Vec<u8> = if archive.len() >= 4 && &archive[0..4] == ENCRYPTED_MAGIC {
        let pw = password.ok_or_else(|| {
            BackupError::EncryptionError("archive is sealed; password is required".to_string())
        })?;
        unseal_inner_zip(archive, pw)?
    } else {
        archive.to_vec()
    };

    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(inner)).map_err(BackupError::ZipError)?;

    // Manifest first, so verification happens before we parse the payload.
    let manifest: WorkspaceExportManifest = {
        let mut f = zip.by_name("manifest.json").map_err(|_| {
            BackupError::CorruptedBackup("archive has no manifest.json".to_string())
        })?;
        let mut s = String::new();
        f.read_to_string(&mut s).map_err(BackupError::IoError)?;
        serde_json::from_str(&s).map_err(BackupError::JsonError)?
    };

    let server_schema = env!("NOSDESK_SCHEMA_HASH");
    if manifest.schema_hash != server_schema {
        return Err(BackupError::CorruptedBackup(format!(
            "schema hash mismatch: archive {} vs server {server_schema}; import needs a matching \
             schema so the FK graph and columns line up",
            manifest.schema_hash
        )));
    }

    // Collect entry names first (the archive borrow can't span by_index calls).
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .collect();

    let mut tables: HashMap<String, Vec<Value>> = HashMap::new();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    for name in names {
        if let Some(table) = name
            .strip_prefix("data/")
            .and_then(|n| n.strip_suffix(".json"))
        {
            let mut f = zip.by_name(&name).map_err(BackupError::ZipError)?;
            let mut s = String::new();
            f.read_to_string(&mut s).map_err(BackupError::IoError)?;
            let rows: Vec<Value> = serde_json::from_str(&s).map_err(BackupError::JsonError)?;
            tables.insert(table.to_string(), rows);
        } else if name.starts_with("files/") && !name.ends_with('/') {
            let mut f = zip.by_name(&name).map_err(BackupError::ZipError)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).map_err(BackupError::IoError)?;
            files.push((name, buf));
        }
    }

    Ok(ArchiveContents {
        manifest,
        tables,
        files,
    })
}

/// Discover single-column integer foreign-key edges (child column → `parent.id`)
/// among `tables`, from `pg_constraint`. Only FKs whose referenced column is `id`
/// are returned — those are the integer keys a remap must rewrite. User FKs
/// (referencing `users.uuid`) and composite FKs are intentionally excluded; users
/// are resolved by uuid, not remapped.
pub fn discover_fk_edges(
    conn: &mut crate::db::DbConnection,
    tables: &HashSet<String>,
) -> Result<Vec<FkEdge>, BackupError> {
    #[derive(QueryableByName)]
    struct EdgeRow {
        #[diesel(sql_type = Text)]
        child_table: String,
        #[diesel(sql_type = Text)]
        child_column: String,
        #[diesel(sql_type = Text)]
        parent_table: String,
        #[diesel(sql_type = Text)]
        parent_column: String,
    }

    let rows: Vec<EdgeRow> = sql_query(
        "SELECT con.conrelid::regclass::text AS child_table, \
                child_col.attname AS child_column, \
                con.confrelid::regclass::text AS parent_table, \
                parent_col.attname AS parent_column \
         FROM pg_constraint con \
         JOIN pg_attribute child_col \
           ON child_col.attrelid = con.conrelid AND child_col.attnum = con.conkey[1] \
         JOIN pg_attribute parent_col \
           ON parent_col.attrelid = con.confrelid AND parent_col.attnum = con.confkey[1] \
         WHERE con.contype = 'f' \
           AND con.connamespace = 'public'::regnamespace \
           AND array_length(con.conkey, 1) = 1",
    )
    .load(conn)
    .map_err(BackupError::DatabaseError)?;

    Ok(rows
        .into_iter()
        .filter(|e| e.parent_column == "id")
        .filter(|e| tables.contains(&e.child_table) && tables.contains(&e.parent_table))
        .map(|e| FkEdge {
            child_table: e.child_table,
            child_column: e.child_column,
            parent_table: e.parent_table,
        })
        .collect())
}

/// Topologically order `tables` so a parent inserts before its children, given
/// the integer-FK `edges`. Self-references (a table pointing at itself) are
/// ignored for ordering — they're handled by a per-table second pass during the
/// insert, so they can't create a false cycle. A genuine cross-table cycle
/// returns an error (the caller can fall back to deferred handling).
pub fn topological_order(tables: &[String], edges: &[FkEdge]) -> Result<Vec<String>, BackupError> {
    let set: HashSet<&str> = tables.iter().map(|s| s.as_str()).collect();
    let mut indegree: HashMap<&str, usize> = tables.iter().map(|t| (t.as_str(), 0)).collect();
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();

    for e in edges {
        // child depends on parent; skip self-refs and edges outside the set.
        if e.child_table == e.parent_table {
            continue;
        }
        if !set.contains(e.child_table.as_str()) || !set.contains(e.parent_table.as_str()) {
            continue;
        }
        children
            .entry(e.parent_table.as_str())
            .or_default()
            .push(e.child_table.as_str());
        *indegree.get_mut(e.child_table.as_str()).unwrap() += 1;
    }

    // Kahn's algorithm; drain zero-indegree nodes in a stable (sorted) order so
    // the output is deterministic across runs.
    let mut queue: VecDeque<&str> = {
        let mut zero: Vec<&str> = indegree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(t, _)| *t)
            .collect();
        zero.sort_unstable();
        zero.into_iter().collect()
    };

    let mut order: Vec<String> = Vec::with_capacity(tables.len());
    while let Some(node) = queue.pop_front() {
        order.push(node.to_string());
        if let Some(kids) = children.get(node) {
            let mut ready: Vec<&str> = Vec::new();
            for &child in kids {
                let d = indegree.get_mut(child).unwrap();
                *d -= 1;
                if *d == 0 {
                    ready.push(child);
                }
            }
            ready.sort_unstable();
            for c in ready {
                queue.push_back(c);
            }
        }
    }

    if order.len() != tables.len() {
        return Err(BackupError::CorruptedBackup(
            "foreign-key cycle among tenant tables; cannot topologically order for import"
                .to_string(),
        ));
    }
    Ok(order)
}

/// Caller-supplied import parameters.
pub struct ImportOptions {
    /// Override the workspace slug. Default keeps the exported slug (region move
    /// preserves identity); an override is needed to import into the same DB,
    /// where the slug is unique.
    pub slug_override: Option<String>,
    /// Override the workspace uuid. Same rationale as the slug.
    pub uuid_override: Option<String>,
    /// Directory to restore uploaded files into (the server's uploads dir in
    /// production; a temp dir in tests).
    pub uploads_dir: PathBuf,
    /// Mint fresh `uuid`s for every imported row instead of preserving them.
    /// Default (false) preserves uuids, which is correct for a cross-DB region
    /// migration (identity continuity, no collision). Set true to clone a
    /// workspace into the SAME database, where the globally-unique uuids would
    /// otherwise collide. Only the row's own `uuid` identity column is minted;
    /// `*_uuid` foreign keys (e.g. user references) are always preserved.
    pub regenerate_uuids: bool,
}

/// Summary of a completed import.
pub struct ImportResult {
    pub workspace_id: i32,
    pub slug: String,
    pub tables_imported: usize,
    pub rows_imported: i64,
    pub files_restored: i64,
}

/// Import a workspace from an export archive into THIS database as a new
/// workspace. Must be called on a BYPASSRLS connection (via
/// `with_actor_bypass_context` / `PlatformConn`). Atomic: everything runs in one
/// transaction and rolls back on any error.
pub fn import_workspace(
    conn: &mut crate::db::DbConnection,
    archive: &[u8],
    password: Option<&str>,
    opts: ImportOptions,
) -> Result<ImportResult, BackupError> {
    let contents = read_archive(archive, password)?;
    let old_ws_id = contents.manifest.workspace_id;

    conn.transaction::<_, BackupError, _>(|conn| {
        // Suppress the audit-capture triggers for this bulk load; both
        // audit_log_trigger and audit_workspace_members short-circuit on this
        // GUC. (Sync events are never generated: the import writes raw, not via
        // sync::emit.) SET LOCAL is scoped to this transaction.
        sql_query("SET LOCAL nosdesk.in_audit_read = 'true'").execute(conn)?;

        let (new_ws_id, slug) = create_target_workspace(conn, &contents, &opts)?;
        upsert_members(conn, &contents)?;

        let tenant_tables: Vec<String> = contents
            .tables
            .keys()
            .filter(|t| t.as_str() != "workspaces" && t.as_str() != "users")
            .cloned()
            .collect();
        let tenant_set: HashSet<String> = tenant_tables.iter().cloned().collect();
        let edges = discover_fk_edges(conn, &tenant_set)?;
        let order = topological_order(&tenant_tables, &edges)?;

        // table -> (old id -> new id), built as we go so a child can look up its
        // already-imported parent.
        let mut id_maps: HashMap<String, HashMap<i64, i64>> = HashMap::new();
        let mut rows_imported = 0i64;
        for table in &order {
            let rows = contents
                .tables
                .get(table)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            rows_imported += remap_insert_table(
                conn,
                table,
                rows,
                &edges,
                new_ws_id,
                opts.regenerate_uuids,
                &mut id_maps,
            )?;
        }

        let files_restored =
            restore_files(&contents.files, old_ws_id, new_ws_id, &opts.uploads_dir)?;

        Ok(ImportResult {
            workspace_id: new_ws_id,
            slug,
            tables_imported: order.len(),
            rows_imported,
            files_restored,
        })
    })
}

/// The public-schema columns of `table` except `id`, in definition order. Used
/// to build an explicit column list so `id` is omitted and drawn from the
/// sequence. Names come from the catalog (safe); callers quote them.
fn table_columns_except_id(
    conn: &mut crate::db::DbConnection,
    table: &str,
) -> Result<Vec<String>, BackupError> {
    #[derive(QueryableByName)]
    struct Col {
        #[diesel(sql_type = Text)]
        column_name: String,
    }
    let rows: Vec<Col> = sql_query(
        "SELECT column_name FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = $1 AND column_name <> 'id' \
         ORDER BY ordinal_position",
    )
    .bind::<Text, _>(table)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    Ok(rows.into_iter().map(|c| c.column_name).collect())
}

/// Insert one JSON row into `table`, omitting `id` (drawn fresh from the
/// sequence) and returning the new id as i64. `table` is validated against the
/// catalog before it's interpolated; columns come from `information_schema`.
fn insert_row_returning_id(
    conn: &mut crate::db::DbConnection,
    table: &str,
    row_json: &str,
) -> Result<i64, BackupError> {
    if !table_exists_in_db(conn, table)? {
        return Err(BackupError::CorruptedBackup(format!(
            "archive references unknown table: {table}"
        )));
    }
    let cols = table_columns_except_id(conn, table)?;
    let col_list = cols
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let query = format!(
        "INSERT INTO \"{table}\" ({col_list}) \
         SELECT {col_list} FROM jsonb_populate_record(NULL::\"{table}\", $1::jsonb) \
         RETURNING id::bigint AS new_id"
    );
    #[derive(QueryableByName)]
    struct IdRow {
        #[diesel(sql_type = BigInt)]
        new_id: i64,
    }
    let r: IdRow = sql_query(&query)
        .bind::<Text, _>(row_json)
        .get_result(conn)
        .map_err(BackupError::DatabaseError)?;
    Ok(r.new_id)
}

/// Create the target workspace from the exported `workspaces` row, taking a fresh
/// id. Slug/uuid are overridable; `custom_domain` and `organisation_id` are
/// dropped (per-cell concerns not carried across a region move).
fn create_target_workspace(
    conn: &mut crate::db::DbConnection,
    contents: &ArchiveContents,
    opts: &ImportOptions,
) -> Result<(i32, String), BackupError> {
    let row = contents
        .tables
        .get("workspaces")
        .and_then(|r| r.first())
        .ok_or_else(|| BackupError::CorruptedBackup("archive has no workspaces row".to_string()))?;
    let obj = row.as_object().ok_or_else(|| {
        BackupError::CorruptedBackup("workspaces row is not an object".to_string())
    })?;
    let mut new_obj = obj.clone();
    if let Some(s) = &opts.slug_override {
        new_obj.insert("slug".to_string(), json!(s));
    }
    if let Some(u) = &opts.uuid_override {
        new_obj.insert("uuid".to_string(), json!(u));
    }
    new_obj.insert("custom_domain".to_string(), Value::Null);
    new_obj.insert("organisation_id".to_string(), Value::Null);
    let slug = new_obj
        .get("slug")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let json = serde_json::to_string(&Value::Object(new_obj)).map_err(BackupError::JsonError)?;
    let new_id = insert_row_returning_id(conn, "workspaces", &json)? as i32;
    Ok((new_id, slug))
}

/// Upsert the exported member users by uuid. Users are global and referenced by
/// uuid (never remapped), so an existing user with the same uuid is kept as-is
/// (`ON CONFLICT (uuid) DO NOTHING`) and only genuinely new users are inserted.
fn upsert_members(
    conn: &mut crate::db::DbConnection,
    contents: &ArchiveContents,
) -> Result<i64, BackupError> {
    let users = match contents.tables.get("users") {
        Some(u) => u,
        None => return Ok(0),
    };
    if !table_exists_in_db(conn, "users")? {
        return Err(BackupError::CorruptedBackup(
            "target has no users table".to_string(),
        ));
    }
    let cols = table_columns_except_id(conn, "users")?;
    let col_list = cols
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let query = format!(
        "INSERT INTO users ({col_list}) \
         SELECT {col_list} FROM jsonb_populate_record(NULL::users, $1::jsonb) \
         ON CONFLICT (uuid) DO NOTHING"
    );
    let mut n = 0i64;
    for user in users {
        let json = serde_json::to_string(user).map_err(BackupError::JsonError)?;
        sql_query(&query)
            .bind::<Text, _>(json)
            .execute(conn)
            .map_err(BackupError::DatabaseError)?;
        n += 1;
    }
    Ok(n)
}

/// Insert one tenant table's rows with id remap. Parents are already imported
/// (topological order), so inter-table integer FKs rewrite via their maps;
/// `workspace_id` is set to the new workspace; self-references are nulled then
/// fixed in a second pass once this table's own old→new map is complete.
fn remap_insert_table(
    conn: &mut crate::db::DbConnection,
    table: &str,
    rows: &[Value],
    edges: &[FkEdge],
    new_ws_id: i32,
    regenerate_uuids: bool,
    id_maps: &mut HashMap<String, HashMap<i64, i64>>,
) -> Result<i64, BackupError> {
    let fk_cols: Vec<(String, String)> = edges
        .iter()
        .filter(|e| e.child_table == table && e.parent_table != table)
        .map(|e| (e.child_column.clone(), e.parent_table.clone()))
        .collect();
    let self_ref_cols: Vec<String> = edges
        .iter()
        .filter(|e| e.child_table == table && e.parent_table == table)
        .map(|e| e.child_column.clone())
        .collect();

    let mut this_map: HashMap<i64, i64> = HashMap::new();
    // (new id, [(self-ref column, old referenced id)]) for the second pass.
    let mut pending_self: Vec<(i64, Vec<(String, i64)>)> = Vec::new();

    for row in rows {
        let obj = row
            .as_object()
            .ok_or_else(|| BackupError::CorruptedBackup(format!("{table} row is not an object")))?;
        let old_id = obj.get("id").and_then(Value::as_i64).ok_or_else(|| {
            BackupError::CorruptedBackup(format!("{table} row missing integer id"))
        })?;
        let mut new_obj = obj.clone();

        if new_obj.contains_key("workspace_id") {
            new_obj.insert("workspace_id".to_string(), json!(new_ws_id));
        }

        // Mint a fresh identity uuid for same-DB clones. Only the row's own
        // `uuid` column; `*_uuid` FK columns are left untouched.
        if regenerate_uuids {
            if let Some(v) = new_obj.get("uuid") {
                if v.is_string() {
                    new_obj.insert("uuid".to_string(), json!(uuid::Uuid::new_v4().to_string()));
                }
            }
        }

        for (col, parent) in &fk_cols {
            if let Some(old_fk) = new_obj.get(col).and_then(Value::as_i64) {
                let parent_map = id_maps.get(parent).ok_or_else(|| {
                    BackupError::CorruptedBackup(format!(
                        "{table}.{col}: parent {parent} not imported before child"
                    ))
                })?;
                let new_fk = *parent_map.get(&old_fk).ok_or_else(|| {
                    BackupError::CorruptedBackup(format!(
                        "{table}.{col} references {parent} id {old_fk} absent from the export"
                    ))
                })?;
                new_obj.insert(col.clone(), json!(new_fk));
            }
        }

        let mut selfs: Vec<(String, i64)> = Vec::new();
        for col in &self_ref_cols {
            if let Some(old_ref) = new_obj.get(col).and_then(Value::as_i64) {
                selfs.push((col.clone(), old_ref));
                new_obj.insert(col.clone(), Value::Null);
            }
        }

        let json =
            serde_json::to_string(&Value::Object(new_obj)).map_err(BackupError::JsonError)?;
        let new_id = insert_row_returning_id(conn, table, &json)?;
        this_map.insert(old_id, new_id);
        if !selfs.is_empty() {
            pending_self.push((new_id, selfs));
        }
    }

    for (new_id, selfs) in &pending_self {
        for (col, old_ref) in selfs {
            let new_ref = *this_map.get(old_ref).ok_or_else(|| {
                BackupError::CorruptedBackup(format!(
                    "{table}.{col} self-reference {old_ref} absent from the export"
                ))
            })?;
            let q = format!("UPDATE \"{table}\" SET \"{col}\" = $1 WHERE id = $2");
            sql_query(&q)
                .bind::<BigInt, _>(new_ref)
                .bind::<BigInt, _>(*new_id)
                .execute(conn)
                .map_err(BackupError::DatabaseError)?;
        }
    }

    let n = rows.len() as i64;
    id_maps.insert(table.to_string(), this_map);
    Ok(n)
}

/// Restore the archive's files to the target's `ws/{new_id}/` prefix, rewriting
/// the source `ws/{old_id}/` path. Files outside the workspace prefix or with a
/// traversal segment are skipped (defence in depth).
fn restore_files(
    files: &[(String, Vec<u8>)],
    old_ws_id: i32,
    new_ws_id: i32,
    uploads_dir: &Path,
) -> Result<i64, BackupError> {
    let old_prefix = format!("files/ws/{old_ws_id}/");
    let new_root = uploads_dir.join("ws").join(new_ws_id.to_string());
    let mut n = 0i64;
    for (archive_path, bytes) in files {
        let rest = match archive_path.strip_prefix(&old_prefix) {
            Some(r) => r,
            None => continue,
        };
        if rest.is_empty() || rest.contains("..") {
            continue;
        }
        let dest = new_root.join(rest);
        if !dest.starts_with(&new_root) {
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(BackupError::IoError)?;
        }
        std::fs::write(&dest, bytes).map_err(BackupError::IoError)?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(child: &str, col: &str, parent: &str) -> FkEdge {
        FkEdge {
            child_table: child.to_string(),
            child_column: col.to_string(),
            parent_table: parent.to_string(),
        }
    }

    #[test]
    fn topological_order_puts_parents_before_children() {
        // comments -> tickets -> workspaces; attachments -> comments.
        let tables = vec![
            "attachments".to_string(),
            "comments".to_string(),
            "tickets".to_string(),
            "workspaces".to_string(),
        ];
        let edges = vec![
            edge("comments", "ticket_id", "tickets"),
            edge("tickets", "workspace_id", "workspaces"),
            edge("attachments", "comment_id", "comments"),
        ];
        let order = topological_order(&tables, &edges).expect("acyclic");
        let pos = |t: &str| order.iter().position(|x| x == t).unwrap();
        assert!(pos("workspaces") < pos("tickets"));
        assert!(pos("tickets") < pos("comments"));
        assert!(pos("comments") < pos("attachments"));
        assert_eq!(order.len(), 4);
    }

    #[test]
    fn topological_order_ignores_self_references() {
        // A self-ref must not count as a cycle.
        let tables = vec!["comments".to_string()];
        let edges = vec![edge("comments", "parent_id", "comments")];
        let order = topological_order(&tables, &edges).expect("self-ref is not a cycle");
        assert_eq!(order, vec!["comments".to_string()]);
    }

    #[test]
    fn topological_order_rejects_a_real_cycle() {
        let tables = vec!["a".to_string(), "b".to_string()];
        let edges = vec![edge("a", "b_id", "b"), edge("b", "a_id", "a")];
        assert!(
            topological_order(&tables, &edges).is_err(),
            "cross-table cycle rejected"
        );
    }

    #[test]
    fn round_trip_export_then_import_remaps_ids() {
        use crate::services::workspace_export::export_workspace;
        use crate::sync::actor::ActorContext;
        use crate::sync::session::with_actor_bypass_context;
        use crate::test_helpers::setup_test_pool;
        use diesel::sql_types::{BigInt, Integer};

        fn as_diesel<T>(r: Result<T, BackupError>) -> Result<T, diesel::result::Error> {
            r.map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
        }

        #[derive(QueryableByName)]
        struct IdRow {
            #[diesel(sql_type = Integer)]
            id: i32,
        }
        #[derive(QueryableByName)]
        struct Cnt {
            #[diesel(sql_type = BigInt)]
            n: i64,
        }

        let pool = setup_test_pool();
        let mut conn = pool.get().expect("test pool connection");
        let actor = ActorContext::system("test:workspace_import");

        let src: i32 = with_actor_bypass_context(&mut conn, &actor, |c| {
            let r: IdRow =
                sql_query("SELECT id FROM workspaces ORDER BY id LIMIT 1").get_result(c)?;
            Ok::<_, diesel::result::Error>(r.id)
        })
        .expect("a workspace exists in the test DB");

        // Export the source workspace (plaintext).
        let archive = with_actor_bypass_context(&mut conn, &actor, |c| {
            as_diesel(export_workspace(c, src, None))
        })
        .expect("export succeeds");

        // Import it back as a NEW workspace (slug + uuid overridden to dodge the
        // same-DB unique collision).
        let unique = std::process::id();
        let tmp = std::env::temp_dir().join(format!("nosdesk-import-{unique}"));
        let opts = ImportOptions {
            slug_override: Some(format!("imported-{unique}")),
            uuid_override: Some("00000000-0000-0000-0000-0000000000ff".to_string()),
            uploads_dir: tmp.clone(),
            // Same-DB round trip: mint fresh row uuids so the globally-unique
            // identity columns don't collide with the source workspace's rows.
            regenerate_uuids: true,
        };
        let result = with_actor_bypass_context(&mut conn, &actor, |c| {
            as_diesel(import_workspace(c, &archive, None, opts))
        })
        .expect("import succeeds (FK graph orders + remaps cleanly)");

        assert_ne!(
            result.workspace_id, src,
            "imported workspace gets a fresh id"
        );
        assert!(result.workspace_id > 0);
        assert_eq!(result.slug, format!("imported-{unique}"));

        // Per-table counts match the source, and the imported rows are scoped to
        // the NEW workspace id — proving the workspace_id rewrite landed.
        let (src_n, new_n): (i64, i64) = with_actor_bypass_context(&mut conn, &actor, |c| {
            let s: Cnt =
                sql_query("SELECT count(*) AS n FROM workflow_states WHERE workspace_id = $1")
                    .bind::<Integer, _>(src)
                    .get_result(c)?;
            let d: Cnt =
                sql_query("SELECT count(*) AS n FROM workflow_states WHERE workspace_id = $1")
                    .bind::<Integer, _>(result.workspace_id)
                    .get_result(c)?;
            Ok::<_, diesel::result::Error>((s.n, d.n))
        })
        .expect("count query");
        assert!(src_n > 0, "source workspace has seeded workflow states");
        assert_eq!(
            src_n, new_n,
            "workflow_states row count preserved by the import"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
