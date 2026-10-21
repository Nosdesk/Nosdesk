//! Workspace import: reads an export archive (see `workspace_export`) and
//! loads it through the import plan (`workspace_archive::plan`).
//!
//! [`read_archive`] unseals, verifies and version-checks the archive. The
//! import into an existing workspace is `workspace_archive::apply`.
//! [`import_workspace`] loads it as a NEW workspace: it creates the workspace
//! row, plans the import into it (people map, keys, collisions), then writes
//! the plan's people and rows. Every integer and uuid key is drawn afresh and
//! rewritten across the foreign-key graph; every user reference goes through
//! the people map, which never attaches an existing account by its archive
//! uuid. The whole import runs in one transaction and rolls back on any error.
//!
//! Execution model: the import runs on a BYPASSRLS connection (the caller wraps
//! it in `with_actor_bypass_context` / `PlatformConn`) and sets
//! `nosdesk.in_audit_read = 'true'` for the transaction, which short-circuits
//! both audit-capture triggers. Sync events are never generated (the import
//! writes raw, not through `sync::emit`), and FK integrity comes from the
//! plan's load order, so no `session_replication_role` or superuser is needed.
//!
//! Known assumption: an integer FK from a tenant table to a table OUTSIDE the
//! import set (a global lookup like `notification_types`, seeded with fixed ids
//! by the migrations) keeps its original id. The version gate makes that hold:
//! both servers run the same migrations.

use std::collections::HashMap;
use std::io::Read;

use diesel::deserialize::QueryableByName;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{BigInt, Text};
use serde_json::{json, Value};

use crate::middleware::workspace_context::DeploymentMode;
use crate::services::backup::{sha256_hex, table_exists_in_db, unseal_inner_zip, BackupError};
use crate::services::workspace_archive::plan::{self, ImportReport, PlanRefusal, Runner};
use crate::services::workspace_archive::{files, rows};
use crate::services::workspace_export::WorkspaceExportManifest;

pub use crate::services::workspace_archive::plan::{discover_fk_edges, topological_order, FkEdge};

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

/// Why an export can't be imported here. A workspace moves only between
/// servers at the same schema, so each refusal names the Nosdesk release to
/// run on one side or the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportRefusal {
    /// Format 1 (Nosdesk 1.1.2 and earlier) carries no email addresses, so
    /// nobody it brings could sign in.
    FormatTooOld { export_version: String },
    /// Made at an earlier schema than this server's, by `release`.
    Earlier { release: String },
    /// Made by a later Nosdesk than this server.
    Newer { export_version: String },
    /// Made at a schema this server doesn't know: a development build, or
    /// migrations edited after they shipped.
    Unknown { export_version: String },
    /// The import plan refuses it (`workspace_archive::plan::PlanRefusal`).
    Plan(Vec<PlanRefusal>),
}

impl std::fmt::Display for ImportRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let current = env!("CARGO_PKG_VERSION");
        match self {
            ImportRefusal::FormatTooOld { export_version } => write!(
                f,
                "This export was made by Nosdesk {export_version}, in an older format that \
                 doesn't carry people's email addresses. Update the server it came from to \
                 Nosdesk {current}, then export the workspace again."
            ),
            ImportRefusal::Earlier { release } => write!(
                f,
                "This export was made by Nosdesk {release}, and this server runs Nosdesk \
                 {current}. Both servers need the same version: update the server it came from \
                 to Nosdesk {current} and export again, or import it on a server running \
                 Nosdesk {release}."
            ),
            ImportRefusal::Newer { export_version } => write!(
                f,
                "This export was made by Nosdesk {export_version}, which is newer than this \
                 server (Nosdesk {current}). Update this server to Nosdesk {export_version}, \
                 then import it."
            ),
            ImportRefusal::Unknown { export_version } => write!(
                f,
                "This export (Nosdesk {export_version}) has a database layout this server \
                 doesn't recognise, as a development build's does. Export the workspace again \
                 from a server running Nosdesk {current}."
            ),
            ImportRefusal::Plan(refusals) => {
                let messages: Vec<String> = refusals.iter().map(|r| r.to_string()).collect();
                f.write_str(&messages.join(" "))
            }
        }
    }
}

/// Whether an export with `manifest` imports here: format 2 or later, made at
/// this build's schema (`backup_upgrade::schema_point_of`).
pub fn check_importable(manifest: &WorkspaceExportManifest) -> Result<(), ImportRefusal> {
    use crate::services::backup_upgrade::{self, NotRestorable, SchemaMarker, SchemaPoint};
    use crate::services::workspace_export::WORKSPACE_EXPORT_FORMAT_VERSION;

    let export_version = manifest.nosdesk_version.clone();
    let format = manifest.workspace_export_format_version;
    if format < WORKSPACE_EXPORT_FORMAT_VERSION {
        return Err(ImportRefusal::FormatTooOld { export_version });
    }
    if format > WORKSPACE_EXPORT_FORMAT_VERSION {
        return Err(ImportRefusal::Newer { export_version });
    }
    let point = backup_upgrade::schema_point_of(SchemaMarker {
        nosdesk_version: &manifest.nosdesk_version,
        schema_hash: &manifest.schema_hash,
        migration_version: manifest.migration_version.as_deref(),
        schema_sha256: manifest.schema_sha256.as_deref(),
    });
    match point {
        Ok(SchemaPoint::Current) => Ok(()),
        Ok(SchemaPoint::Earlier { migration }) => Err(ImportRefusal::Earlier {
            release: backup_upgrade::release_at(migration)
                .map(str::to_string)
                .unwrap_or(export_version),
        }),
        Err(NotRestorable::Newer { .. }) => Err(ImportRefusal::Newer { export_version }),
        Err(NotRestorable::Unknown { .. }) => Err(ImportRefusal::Unknown { export_version }),
    }
}

/// Read + verify an export archive from bytes. Unseals when the `NODB` envelope
/// is present (password required), then unzips and parses the manifest, the
/// per-table row arrays, and the file blobs. Refuses an export from another
/// format or schema ([`check_importable`]): a different schema means a
/// different FK graph and column set, so a remap would be unsafe.
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

    check_importable(&manifest).map_err(BackupError::NotImportable)?;

    // Collect entry names first (the archive borrow can't span by_index calls).
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| {
            let f = zip.by_index(i).ok()?;
            f.name().ok().map(|n| n.to_string())
        })
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
            // Integrity: the payload must match the manifest's per-table hash, so
            // silent corruption of a data/*.json entry is caught before it's
            // inserted (the manifest itself is under the AES-GCM envelope).
            if let Some(tm) = manifest.tables.get(table) {
                let actual = sha256_hex(s.as_bytes());
                if actual != tm.sha256 {
                    return Err(BackupError::CorruptedBackup(format!(
                        "checksum mismatch for data/{table}.json (archive corrupted)"
                    )));
                }
            }
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

/// Caller-supplied import parameters.
pub struct ImportOptions {
    /// Override the workspace slug. Default keeps the exported slug (region move
    /// preserves identity); an override is needed to import into the same DB,
    /// where the slug is unique.
    pub slug_override: Option<String>,
    /// Override the workspace uuid. Same rationale as the slug.
    pub uuid_override: Option<String>,
    /// Mint fresh `uuid`s for every imported row instead of preserving them.
    /// Default (false) preserves uuids, which is correct for a cross-DB region
    /// migration (identity continuity, no collision). Set true to clone a
    /// workspace into the SAME database, where the globally-unique uuids would
    /// otherwise collide. Only the row's own `uuid` identity column is minted;
    /// User references go through the people map either way.
    pub regenerate_uuids: bool,
    /// Which mapping rules people follow. Explicit, never read from the
    /// environment here.
    pub mode: DeploymentMode,
    /// Who runs the import: only their own emails merge into their account.
    pub runner: Runner,
}

/// Summary of a completed import. File restore happens separately in the handler
/// (async, through the storage abstraction), so its count isn't reported here.
pub struct ImportResult {
    pub workspace_id: i32,
    pub slug: String,
    pub tables_imported: usize,
    pub rows_imported: i64,
    /// The plan's report: per-table counts, the people map, handover items.
    pub report: ImportReport,
    /// Where the archive's files land, for `workspace_archive::files::restore`.
    pub files: crate::services::workspace_archive::files::FilePlan,
}

/// Import a workspace's ROWS from an already-read archive into THIS database as a
/// new workspace. Must be called on a BYPASSRLS connection (via
/// `with_actor_bypass_context` / `PlatformConn`). Atomic: everything runs in one
/// transaction and rolls back on any error. The caller reads the archive
/// ([`read_archive`]) and, after this returns, restores the archive's files
/// through the storage abstraction (async, `workspace_archive::files::restore`
/// with [`ImportResult::files`]) so file migration works on local and S3.
///
/// A plan refusal comes back as [`BackupError::NotImportable`] with
/// [`ImportRefusal::Plan`].
pub fn import_workspace(
    conn: &mut crate::db::DbConnection,
    contents: &ArchiveContents,
    opts: &ImportOptions,
) -> Result<ImportResult, BackupError> {
    conn.transaction::<_, BackupError, _>(|conn| {
        // Suppress the audit-capture triggers for this bulk load; both
        // audit_log_trigger and audit_workspace_members short-circuit on this
        // GUC. (Sync events are never generated: the import writes raw, not via
        // sync::emit.) SET LOCAL is scoped to this transaction.
        sql_query("SET LOCAL nosdesk.in_audit_read = 'true'").execute(conn)?;

        // The archive decides which tables get written, and this runs BYPASSRLS
        // with the audit triggers suppressed above, so an archive naming a
        // global or platform table is refused before anything is written. The
        // archive is untrusted input even behind the platform-admin gate.
        let outside = plan::tables_outside_workspace(conn, contents)?;
        if !outside.is_empty() {
            return Err(BackupError::CorruptedBackup(format!(
                "archive names tables that are not workspace-scoped: {}",
                outside.join(", ")
            )));
        }

        let (workspace_id, slug) = create_target_workspace(conn, contents, opts)?;
        let target = plan::ImportTarget {
            workspace_id,
            mode: opts.mode,
            runner: opts.runner.clone(),
        };
        let mut plan = plan::plan_import(
            conn,
            contents,
            &target,
            plan::PlanOptions {
                regenerate_row_uuids: opts.regenerate_uuids,
            },
        )?;
        if !plan.importable() {
            return Err(BackupError::NotImportable(ImportRefusal::Plan(
                plan.report.refusals.clone(),
            )));
        }
        let alloc = plan.allocate(conn)?;
        let files = files::FilePlan::build(contents, &alloc.lookup());
        let (tables_imported, rows_imported) =
            crate::services::workspace_archive::apply::load(conn, &mut plan, &alloc)?;
        let workspace_uuid = crate::repository::workspaces::find_by_id(conn, workspace_id)?
            .ok_or_else(|| {
                BackupError::CorruptedBackup("the imported workspace is missing".to_string())
            })?
            .uuid;
        files::settle_branding(conn, workspace_id, workspace_uuid, contents, &files, None)?;

        Ok(ImportResult {
            workspace_id,
            slug,
            tables_imported,
            rows_imported,
            report: plan.report,
            files,
        })
    })
}

/// Insert one archive row into `table`, omitting `id` (drawn fresh from the
/// sequence) and returning the new id as i64; `None` when the column policy
/// skips the row.
fn insert_row_returning_id(
    conn: &mut crate::db::DbConnection,
    table: &str,
    row: &serde_json::Map<String, Value>,
) -> Result<Option<i64>, BackupError> {
    #[derive(QueryableByName)]
    struct IdRow {
        #[diesel(sql_type = BigInt)]
        new_id: i64,
    }
    if !table_exists_in_db(conn, table)? {
        return Err(BackupError::CorruptedBackup(format!(
            "archive references unknown table: {table}"
        )));
    }
    let keys = row.keys().cloned().collect();
    let query = rows::load_statement(
        conn,
        table,
        &keys,
        &["id"],
        "RETURNING id::bigint AS new_id",
    )?;
    let payload = serde_json::to_string(&[row]).map_err(BackupError::JsonError)?;
    let ids: Vec<IdRow> = sql_query(&query)
        .bind::<Text, _>(payload)
        .load(conn)
        .map_err(BackupError::DatabaseError)?;
    Ok(ids.first().map(|r| r.new_id))
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
    // The workspace's own uuid: an explicit override wins; otherwise mint a fresh
    // one under regenerate_uuids so a same-DB clone doesn't hit workspaces_uuid_key
    // (the tenant rows are regenerated the same way).
    if let Some(u) = &opts.uuid_override {
        new_obj.insert("uuid".to_string(), json!(u));
    } else if opts.regenerate_uuids {
        new_obj.insert("uuid".to_string(), json!(uuid::Uuid::new_v4().to_string()));
    }
    new_obj.insert("custom_domain".to_string(), Value::Null);
    new_obj.insert("organisation_id".to_string(), Value::Null);
    let slug = new_obj
        .get("slug")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let new_id = insert_row_returning_id(conn, "workspaces", &new_obj)?.ok_or_else(|| {
        BackupError::CorruptedBackup("the workspaces row did not load".to_string())
    })? as i32;
    Ok((new_id, slug))
}

/// The plan refusals in `err`, if it is one.
pub fn plan_refusals(err: &BackupError) -> Option<&[PlanRefusal]> {
    match err {
        BackupError::NotImportable(ImportRefusal::Plan(refusals)) => Some(refusals),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn export_manifest(
        format: u32,
        nosdesk_version: &str,
        schema_hash: &str,
        point: Option<(&str, &str)>,
    ) -> WorkspaceExportManifest {
        WorkspaceExportManifest {
            workspace_export_format_version: format,
            nosdesk_version: nosdesk_version.to_string(),
            schema_hash: schema_hash.to_string(),
            migration_version: point.map(|(v, _)| v.to_string()),
            schema_sha256: point.map(|(_, s)| s.to_string()),
            created_at: String::new(),
            workspace_id: 1,
            workspace_slug: "acme".to_string(),
            workspace_uuid: String::new(),
            member_user_uuids: Vec::new(),
            tables: HashMap::new(),
            files: Default::default(),
            avatars: Default::default(),
            source_mode: "self_hosted".to_string(),
            source_origin: None,
            custom_domain: None,
            sealed: false,
            secrets: false,
        }
    }

    fn prefix_at(version: &str) -> (&'static str, &'static str) {
        let p = crate::services::backup_upgrade::MIGRATION_PREFIXES
            .iter()
            .find(|p| p.version == version)
            .expect("a shipped migration point");
        (p.version, p.sha256)
    }

    const CURRENT: &str = env!("CARGO_PKG_VERSION");

    /// An export at this build's schema imports, whatever the
    /// toolchain-dependent fingerprint says.
    #[test]
    fn an_export_at_this_schema_imports() {
        let current = crate::services::backup_upgrade::current_schema();
        let m = export_manifest(
            2,
            CURRENT,
            "from-another-toolchain",
            Some((current.version, current.sha256)),
        );
        assert_eq!(check_importable(&m), Ok(()));
    }

    /// Format 1 carries no emails, so even one at this schema is refused,
    /// and the message names the release to export from.
    #[test]
    fn a_format_1_export_is_refused() {
        let current = crate::services::backup_upgrade::current_schema();
        let m = export_manifest(
            1,
            "1.1.2",
            env!("NOSDESK_SCHEMA_HASH"),
            Some((current.version, current.sha256)),
        );
        let refused = check_importable(&m).expect_err("format 1");
        assert_eq!(
            refused,
            ImportRefusal::FormatTooOld {
                export_version: "1.1.2".to_string()
            }
        );
        let message = refused.to_string();
        assert!(message.contains("older format"), "{message}");
        assert!(
            message.contains(&format!(
                "Update the server it came from to Nosdesk {CURRENT}"
            )),
            "{message}"
        );
    }

    /// An export from an earlier release names that release, from the
    /// pinned schema points rather than the version the build reported.
    #[test]
    fn an_earlier_export_is_refused_naming_its_release() {
        let m = export_manifest(2, "1.1.1-dev", "ignored", Some(prefix_at("20261014060000")));
        let refused = check_importable(&m).expect_err("earlier schema");
        assert_eq!(
            refused,
            ImportRefusal::Earlier {
                release: "1.1.1".to_string()
            }
        );
        let message = refused.to_string();
        assert!(
            message.contains("made by Nosdesk 1.1.1, and this server runs"),
            "{message}"
        );
        assert!(
            message.contains(&format!("to Nosdesk {CURRENT} and export again")),
            "{message}"
        );
        assert!(
            message.contains("on a server running Nosdesk 1.1.1"),
            "{message}"
        );

        // A point no release pinned (a build between releases) names the
        // version that build reported.
        let earlier = crate::services::backup_upgrade::MIGRATION_PREFIXES[1];
        let m = export_manifest(
            2,
            "1.0.9",
            "ignored",
            Some((earlier.version, earlier.sha256)),
        );
        assert_eq!(
            check_importable(&m),
            Err(ImportRefusal::Earlier {
                release: "1.0.9".to_string()
            })
        );
    }

    #[test]
    fn a_newer_export_is_refused_naming_its_release() {
        let m = export_manifest(2, "99.0.0", "ignored", Some(("99990101000000", "abc")));
        let refused = check_importable(&m).expect_err("newer schema");
        assert_eq!(
            refused,
            ImportRefusal::Newer {
                export_version: "99.0.0".to_string()
            }
        );
        let message = refused.to_string();
        assert!(
            message.contains("Update this server to Nosdesk 99.0.0"),
            "{message}"
        );
        // A later format is a later Nosdesk too.
        let current = crate::services::backup_upgrade::current_schema();
        let m = export_manifest(
            3,
            "99.0.0",
            "ignored",
            Some((current.version, current.sha256)),
        );
        assert!(matches!(
            check_importable(&m),
            Err(ImportRefusal::Newer { .. })
        ));
    }

    #[test]
    fn an_unknown_schema_is_refused() {
        let current = crate::services::backup_upgrade::current_schema();
        // This build's last migration with other contents: edited, or a
        // development build.
        let m = export_manifest(2, CURRENT, "ignored", Some((current.version, "0000")));
        let refused = check_importable(&m).expect_err("unknown schema");
        assert_eq!(
            refused,
            ImportRefusal::Unknown {
                export_version: CURRENT.to_string()
            }
        );
        let message = refused.to_string();
        assert!(message.contains("doesn't recognise"), "{message}");
        assert!(
            message.contains(&format!("from a server running Nosdesk {CURRENT}")),
            "{message}"
        );
    }

    /// `read_archive` refuses before reading any rows, with the refusal as
    /// the whole message.
    #[test]
    fn read_archive_refuses_a_format_1_export() {
        use std::io::Write;
        let current = crate::services::backup_upgrade::current_schema();
        let m = export_manifest(
            1,
            "1.1.2",
            env!("NOSDESK_SCHEMA_HASH"),
            Some((current.version, current.sha256)),
        );
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(serde_json::to_string(&m).unwrap().as_bytes())
            .unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let err = read_archive(&bytes, None)
            .err()
            .expect("format 1 is refused");
        assert!(matches!(
            err,
            BackupError::NotImportable(ImportRefusal::FormatTooOld { .. })
        ));
        assert!(err
            .to_string()
            .starts_with("This export was made by Nosdesk 1.1.2"));
    }

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

        // Seed a composite-PK junction row (user_groups has no `id` column) that
        // references a user who is NOT a workspace member. This exercises two of
        // the audit's findings at once: the id-less insert path, and exporting a
        // referenced non-member user (user_groups.user_uuid FKs users.uuid, so the
        // import FK-fails unless the export pulled that user in). in_audit_read
        // suppresses the audit trigger for the seed (no workspace pin here).
        #[derive(QueryableByName)]
        struct UuidRow {
            #[diesel(sql_type = Text)]
            uuid: String,
        }
        let referenced_uuid: String = with_actor_bypass_context(&mut conn, &actor, |c| {
            sql_query("SET LOCAL nosdesk.in_audit_read = 'true'").execute(c)?;
            let u: UuidRow = sql_query(
                "INSERT INTO users (name) VALUES ('audit-test-user') RETURNING uuid::text AS uuid",
            )
            .get_result(c)?;
            let g: IdRow = sql_query(
                "INSERT INTO groups \
                   (uuid, name, created_at, updated_at, mail_enabled, security_enabled, \
                    sync_enabled, workspace_id) \
                 VALUES (gen_random_uuid(), 'audit-test-group', now(), now(), false, false, \
                    false, $1) RETURNING id",
            )
            .bind::<Integer, _>(src)
            .get_result(c)?;
            sql_query(
                "INSERT INTO user_groups (user_uuid, group_id, created_at, workspace_id) \
                 VALUES ($1::uuid, $2, now(), $3)",
            )
            .bind::<Text, _>(&u.uuid)
            .bind::<Integer, _>(g.id)
            .bind::<Integer, _>(src)
            .execute(c)?;
            Ok::<_, diesel::result::Error>(u.uuid)
        })
        .expect("seed junction row + referenced non-member user");

        // Export the source workspace (plaintext).
        let archive = with_actor_bypass_context(&mut conn, &actor, |c| {
            as_diesel(export_workspace(c, src, None))
        })
        .expect("export succeeds");

        // Import it back as a NEW workspace (slug + uuid overridden to dodge the
        // same-DB unique collision).
        let unique = std::process::id();
        let opts = ImportOptions {
            slug_override: Some(format!("imported-{unique}")),
            uuid_override: Some("00000000-0000-0000-0000-0000000000ff".to_string()),
            // Same-DB round trip: mint fresh row uuids so the globally-unique
            // identity columns don't collide with the source workspace's rows.
            regenerate_uuids: true,
            mode: DeploymentMode::SelfHosted,
            runner: Runner::Operator { user: None },
        };
        let contents = read_archive(&archive, None).expect("read archive");

        // The referenced non-member user must be in the exported users dump, or
        // the import would fail the user_groups FK.
        let users_dump = contents.tables.get("users").expect("users dump present");
        assert!(
            users_dump
                .iter()
                .any(|u| u.get("uuid").and_then(|v| v.as_str()) == Some(referenced_uuid.as_str())),
            "export must include a user referenced by a row even if not a member"
        );

        let result = with_actor_bypass_context(&mut conn, &actor, |c| {
            as_diesel(import_workspace(c, &contents, &opts))
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

        // The id-less junction row survived the round trip (regression guard for
        // the audit's critical finding: composite-PK tables have no `id`).
        let new_ug: i64 = with_actor_bypass_context(&mut conn, &actor, |c| {
            let d: Cnt = sql_query("SELECT count(*) AS n FROM user_groups WHERE workspace_id = $1")
                .bind::<Integer, _>(result.workspace_id)
                .get_result(c)?;
            Ok::<_, diesel::result::Error>(d.n)
        })
        .expect("junction count query");
        assert_eq!(
            new_ug, 1,
            "the id-less user_groups junction row was imported into the new workspace"
        );
    }

    /// An archive naming a table that is not workspace-scoped is refused
    /// outright, before anything is written.
    ///
    /// The insert helpers validate that a table EXISTS (so the dynamic SQL is
    /// not injectable) but not that it is a TENANT table. Since the import runs
    /// BYPASSRLS with `nosdesk.in_audit_read` suppressing the audit triggers, a
    /// crafted archive could otherwise write unaudited rows into a global or
    /// platform table. Regression test for S2 (docs/security-audit-2026-08.md).
    #[test]
    fn archive_naming_a_non_tenant_table_is_refused() {
        use crate::services::workspace_export::export_workspace;
        use crate::sync::actor::ActorContext;
        use crate::sync::session::with_actor_bypass_context;
        use crate::test_helpers::setup_test_pool;
        use diesel::sql_types::Integer;

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
            #[diesel(sql_type = diesel::sql_types::BigInt)]
            n: i64,
        }

        let pool = setup_test_pool();
        let mut conn = pool.get().expect("test pool connection");
        let actor = ActorContext::system("test:workspace_import_allowlist");

        let src: i32 = with_actor_bypass_context(&mut conn, &actor, |c| {
            let r: IdRow =
                sql_query("SELECT id FROM workspaces ORDER BY id LIMIT 1").get_result(c)?;
            Ok::<_, diesel::result::Error>(r.id)
        })
        .expect("a workspace exists in the test DB");

        // Start from a genuine export so the manifest and every other table are
        // exactly what the importer expects, then tamper with one entry. That is
        // the realistic shape of this attack: a valid archive with an extra
        // table spliced in, not a hand-built one that fails for other reasons.
        let archive = with_actor_bypass_context(&mut conn, &actor, |c| {
            as_diesel(export_workspace(c, src, None))
        })
        .expect("export succeeds");
        let mut contents = read_archive(&archive, None).expect("read archive");

        // `refresh_tokens` exists but carries no `workspace_id`, so it is
        // outside the exportable/importable set. It is also the sharpest
        // illustration of why the guard matters: a row forged into it is a
        // usable session credential, written with the audit triggers suppressed
        // and outside any tenant.
        let before: i64 = with_actor_bypass_context(&mut conn, &actor, |c| {
            let r: Cnt = sql_query("SELECT count(*) AS n FROM refresh_tokens").get_result(c)?;
            Ok::<_, diesel::result::Error>(r.n)
        })
        .expect("count migrations");

        contents.tables.insert(
            "refresh_tokens".to_string(),
            vec![json!({
                "token_hash": "0000000000000000000000000000000000000000000000000000000000000000",
                "user_uuid": "00000000-0000-0000-0000-000000000001",
                "expires_at": "2099-01-01T00:00:00Z",
            })],
        );

        let unique = std::process::id();
        let opts = ImportOptions {
            slug_override: Some(format!("rejected-{unique}")),
            uuid_override: Some("00000000-0000-0000-0000-0000000000fe".to_string()),
            regenerate_uuids: true,
            mode: DeploymentMode::SelfHosted,
            runner: Runner::Operator { user: None },
        };

        let err = with_actor_bypass_context(&mut conn, &actor, |c| {
            Ok::<_, diesel::result::Error>(import_workspace(c, &contents, &opts).err())
        })
        .expect("the transaction itself runs")
        .expect("import must refuse an archive naming a non-tenant table");

        match err {
            BackupError::CorruptedBackup(msg) => assert!(
                msg.contains("refresh_tokens"),
                "the error should name the offending table, got: {msg}"
            ),
            other => panic!("expected CorruptedBackup, got {other:?}"),
        }

        // Refused before any write: the offending table is untouched and no
        // workspace was created.
        let (after, leaked): (i64, i64) = with_actor_bypass_context(&mut conn, &actor, |c| {
            let a: Cnt = sql_query("SELECT count(*) AS n FROM refresh_tokens").get_result(c)?;
            let w: Cnt = sql_query("SELECT count(*) AS n FROM workspaces WHERE slug = $1")
                .bind::<Text, _>(format!("rejected-{unique}"))
                .get_result(c)?;
            Ok::<_, diesel::result::Error>((a.n, w.n))
        })
        .expect("post-check counts");
        assert_eq!(before, after, "no row was written to the non-tenant table");
        assert_eq!(leaked, 0, "no partial workspace was left behind");
    }
}
