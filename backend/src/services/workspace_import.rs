//! Workspace-scoped import — Phase 2 of the tenant export/import primitive.
//!
//! Reads a Phase 1 export archive (see `workspace_export`) and reconstructs the
//! workspace in THIS database as a NEW workspace with a fresh integer id,
//! remapping every integer primary key and foreign key so it can't collide with
//! the pooled cell's other tenants. Users are matched/upserted by `uuid`
//! (global, never remapped). The whole import runs in one transaction under
//! `session_replication_role = 'replica'` — deferring FK checks and suppressing
//! the audit trigger — the same mechanism the whole-DB restore uses.
//!
//! This file is the FOUNDATION: reading + verifying the archive, discovering the
//! integer-FK graph at runtime, and topologically ordering the tables. The
//! remap-insert (the crux) and file restore build on these.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;

use diesel::deserialize::QueryableByName;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::Text;
use serde_json::Value;

use crate::services::backup::{unseal_inner_zip, BackupError};
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
}
