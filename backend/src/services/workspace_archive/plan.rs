//! A workspace import's plan: every decision the import makes before it
//! writes a row.
//!
//! [`plan_import`] reads the archive and the target, and writes nothing. It
//! decides which tables load (the policy's carried classes), which account
//! each archive person's rows land on ([`Outcome`]), which rows a person rule
//! or a handover item leaves out, and what refuses the import
//! ([`PlanRefusal`]). [`ImportPlan::allocate`] then draws every new key from
//! the target's sequences, and [`ImportPlan::rows`] gives a table's rows with
//! its own key, its foreign keys, every user reference and `workspace_id`
//! rewritten, in an order [`ImportPlan::tables`] can load as given.
//!
//! People: no archive uuid ever selects an existing account. A person maps by
//! their archive primary email under the target's rules; an account the plan
//! creates keeps the archive uuid only when no account on the target has it.
//!
//! The plan carries its own report ([`ImportReport`]): per-table counts, the
//! people map with each outcome, what the import leaves for someone to redo
//! ([`Handover`]), and the refusals. The preview and the CLI render it.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Array, BigInt, Bool, Integer, Nullable, Text};
use serde::Serialize;
use serde_json::{Map, Value};
use uuid::Uuid;

use super::policy::{self, Class};
use crate::db::DbConnection;
use crate::middleware::workspace_context::DeploymentMode;
use crate::services::backup::BackupError;
use crate::services::imports::refusal::Refused;
use crate::services::workspace_import::ArchiveContents;

/// One archive row.
pub type Row = Map<String, Value>;

/// Archive entries the plan reads as people and workspace, not as workspace
/// tables.
pub const PEOPLE_AND_WORKSPACE: &[&str] = &["workspaces", "users", "user_emails"];

/// Whether `table` is one of [`PEOPLE_AND_WORKSPACE`].
pub fn is_people_or_workspace(table: &str) -> bool {
    PEOPLE_AND_WORKSPACE.contains(&table)
}

/// Workspace roles that make an archive person staff.
const STAFF_ROLES: &[&str] = &["owner", "admin", "agent"];

/// Integrations that talk to the outside world, switched off on import:
/// `(table, column, on, off)` as JSON. A row whose column holds `on` gets
/// `off`. Their credentials don't decrypt here, and someone checks each one
/// before it runs against the new server.
pub const SWITCHED_OFF: &[(&str, &str, &str, &str)] = &[
    ("channels", "enabled", "true", "false"),
    ("workspace_email_settings", "enabled", "true", "false"),
    ("workspace_ldap_settings", "enabled", "true", "false"),
    ("workspace_identity_providers", "enabled", "true", "false"),
    (
        "workspace_identity_providers",
        "teams_enabled",
        "true",
        "false",
    ),
    ("workspace_widget_settings", "enabled", "true", "false"),
    ("webhooks", "enabled", "true", "false"),
    ("plugins", "state", "\"installed\"", "\"disabled\""),
];

/// Instance-unique capability tokens a new value replaces on import.
pub const REGENERATED: &[(&str, &str)] = &[("tickets", "guest_lookup_token")];

/// User references with no foreign key: `(table, column)`, each a uuid or a
/// uuid array. They go through the people map like the foreign keys do. The
/// export collects people through foreign keys only, so one of these can name
/// someone the archive doesn't carry: the plan clears it (an array drops it)
/// and counts it, rather than leave a uuid that may be another account's here.
pub const USER_REFERENCES_WITHOUT_FK: &[(&str, &str)] = &[
    ("article_content_revisions", "contributed_by"),
    ("assignment_log", "new_assignee_uuid"),
    ("assignment_log", "previous_assignee_uuid"),
    ("outbound_emails", "skip_if_seen_by"),
    ("ticket_merges", "merged_by_user_uuid"),
];

/// Rows that hold a plugin's secret value, sealed under the source's keyring.
/// The import leaves them out.
pub const PLUGIN_SECRETS: (&str, &str) = ("plugin_data", "is_secret");

// ---------------------------------------------------------------------------
// Target and runner
// ---------------------------------------------------------------------------

/// Who runs the import. Only the runner's own emails merge an archive person
/// into an existing account without matching anyone else's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runner {
    /// The workspace Owner on a self-hosted server: their primary email
    /// merges.
    SelfHostedOwner { user: Uuid },
    /// The workspace owner on hosted, through the control plane: the emails
    /// the control plane verified for them merge.
    HostedOwner {
        user: Uuid,
        verified_emails: Vec<String>,
    },
    /// An operator (CLI or platform admin). On a self-hosted target their
    /// primary email merges; on hosted, nobody's does.
    Operator { user: Option<Uuid> },
}

impl Runner {
    fn user(&self) -> Option<Uuid> {
        match self {
            Runner::SelfHostedOwner { user } | Runner::HostedOwner { user, .. } => Some(*user),
            Runner::Operator { user } => *user,
        }
    }
}

/// The workspace an import fills, and how. The mode is explicit, never read
/// from the environment here, so one process can plan for either.
#[derive(Debug, Clone)]
pub struct ImportTarget {
    pub workspace_id: i32,
    pub mode: DeploymentMode,
    pub runner: Runner,
}

/// Choices the caller makes about the archive's rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlanOptions {
    /// Give every row a new `uuid` instead of the archive's: a copy of a
    /// workspace into the server that still holds it.
    pub regenerate_row_uuids: bool,
}

// ---------------------------------------------------------------------------
// People
// ---------------------------------------------------------------------------

/// Where an archive person's rows land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The runner's own account (their email).
    MergedIntoRunner,
    /// An existing account that holds the person's email.
    Mapped,
    /// A new account with the person's email (if any) and membership.
    Created,
    /// Hosted only: a new account with no email and no membership, which a
    /// staff seat later adopts.
    Placeholder,
}

/// Why a hosted import holds a person as a placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceholderReason {
    /// Staff in the archive: the control plane seats them.
    Staff,
    /// The archive has no email for them.
    NoEmail,
    /// A requester whose email a verified, privileged or SSO-linked account
    /// holds here, so the import doesn't attach their history to it.
    RequesterUnlinked,
}

/// One archive person and where they land.
#[derive(Debug, Clone, Serialize)]
pub struct Person {
    pub archive_uuid: Uuid,
    /// The account their rows reference on the target.
    pub target_uuid: Uuid,
    pub name: String,
    /// Their archive primary email, normalised.
    pub email: Option<String>,
    /// Their role in the archive's workspace, any status.
    pub archive_role: Option<String>,
    pub staff: bool,
    /// Removed from the workspace or deleted in the archive.
    pub disabled: bool,
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<PlaceholderReason>,
    /// Their role on the target after the import: the role they already
    /// hold there, else the archive's (an owner other than the runner comes
    /// in as admin when the target has an owner). `None`: no membership.
    pub role_on_target: Option<String>,
    /// A new account's archive username was taken here, so it has none.
    pub username_dropped: bool,
    /// A new account's archive uuid was taken here, so it has a new one.
    pub uuid_reminted: bool,
    #[serde(skip)]
    username: Option<String>,
    #[serde(skip)]
    email_type: &'static str,
    #[serde(skip)]
    user_row: Row,
}

impl Person {
    /// The plan creates the account (a new user or a placeholder).
    pub fn is_new(&self) -> bool {
        matches!(self.outcome, Outcome::Created | Outcome::Placeholder)
    }
}

/// Totals over the people map.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PeopleCounts {
    pub merged_into_runner: usize,
    pub mapped: usize,
    pub created: usize,
    pub placeholders: usize,
    pub requesters_unlinked: usize,
    pub usernames_dropped: usize,
    pub uuids_reminted: usize,
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

/// Something the import leaves for someone to redo or check (#841's
/// handover). The import applies each in the rows it loads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Handover {
    /// A credential sealed under the source's keyring, left out: enter it
    /// again.
    EncryptedSettingDropped {
        table: String,
        column: String,
        rows: usize,
    },
    /// Instance-unique lookup tokens given new values; earlier links to them
    /// stop working.
    TokensRegenerated {
        table: String,
        column: String,
        rows: usize,
    },
    /// Plugin secret values left out: enter them again.
    PluginSecretsDropped { rows: usize },
    /// An integration that was on arrives off.
    SwitchedOff {
        table: String,
        column: String,
        rows: usize,
    },
}

/// One archive table in the plan.
#[derive(Debug, Clone, Serialize)]
pub struct TableReport {
    pub table: String,
    /// `move`, `replace`, `merge`, `never` or `excluded`.
    pub class: &'static str,
    pub in_archive: usize,
    /// Rows the import loads.
    pub planned: usize,
    /// Rows left out, by reason.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dropped: BTreeMap<&'static str, usize>,
    /// User references without a foreign key that name someone the archive
    /// doesn't carry, cleared, by column ([`USER_REFERENCES_WITHOUT_FK`]).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub cleared: BTreeMap<String, usize>,
}

/// Why an import can't go ahead. Each names the table it's about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum PlanRefusal {
    /// The archive names tables that aren't workspace tables here.
    NotWorkspaceTables { tables: Vec<String> },
    /// Rows whose instance-unique values a row here already has, or that
    /// repeat within the archive.
    Collision {
        table: String,
        columns: Vec<String>,
        rows: usize,
    },
    /// Rows naming a person the archive doesn't carry.
    UnknownPerson {
        table: String,
        column: String,
        rows: usize,
    },
    /// Rows referencing a row the archive doesn't carry.
    MissingParent {
        table: String,
        column: String,
        parent: String,
        rows: usize,
    },
    /// Rows that reference each other in a loop.
    ReferenceCycle { table: String },
    /// The archive lists one person twice.
    DuplicatePerson { uuid: String },
    /// Two archive people share an email.
    DuplicateEmail { email: String },
    /// A row that isn't what its table holds.
    BadRow { table: String, reason: String },
}

impl PlanRefusal {
    /// The code a client words the refusal by.
    pub fn code(&self) -> &'static str {
        match self {
            PlanRefusal::NotWorkspaceTables { .. } => "workspace_import.not_workspace_tables",
            PlanRefusal::Collision { .. } => "workspace_import.collision",
            PlanRefusal::UnknownPerson { .. } => "workspace_import.unknown_person",
            PlanRefusal::MissingParent { .. } => "workspace_import.missing_parent",
            PlanRefusal::ReferenceCycle { .. } => "workspace_import.reference_cycle",
            PlanRefusal::DuplicatePerson { .. } => "workspace_import.duplicate_person",
            PlanRefusal::DuplicateEmail { .. } => "workspace_import.duplicate_email",
            PlanRefusal::BadRow { .. } => "workspace_import.bad_row",
        }
    }

    /// As the import surfaces' shared refusal: code, English, and the values.
    pub fn to_refused(&self) -> Refused {
        let args = match serde_json::to_value(self) {
            Ok(Value::Object(mut args)) => {
                args.remove("code");
                args
            }
            _ => Map::new(),
        };
        let mut refused = Refused::new(self.code(), self.to_string());
        refused.args = args;
        refused
    }
}

impl std::fmt::Display for PlanRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanRefusal::NotWorkspaceTables { tables } => write!(
                f,
                "The export names tables that aren't workspace data: {}.",
                tables.join(", ")
            ),
            PlanRefusal::Collision {
                table,
                columns,
                rows,
            } => write!(
                f,
                "{rows} {table} rows have a {} that this server already holds or that the export \
                 repeats. This server may still hold the workspace the export came from.",
                columns.join(", ")
            ),
            PlanRefusal::UnknownPerson {
                table,
                column,
                rows,
            } => write!(
                f,
                "{rows} {table} rows name a person in {column} that the export doesn't include."
            ),
            PlanRefusal::MissingParent {
                table,
                column,
                parent,
                rows,
            } => write!(
                f,
                "{rows} {table} rows reference {parent} rows (through {column}) that the export \
                 doesn't include."
            ),
            PlanRefusal::ReferenceCycle { table } => {
                write!(
                    f,
                    "{table} rows in the export reference each other in a loop."
                )
            }
            PlanRefusal::DuplicatePerson { uuid } => {
                write!(f, "The export lists the person {uuid} more than once.")
            }
            PlanRefusal::DuplicateEmail { email } => {
                write!(f, "The export gives {email} to more than one person.")
            }
            PlanRefusal::BadRow { table, reason } => {
                write!(f, "A {table} row in the export is malformed: {reason}.")
            }
        }
    }
}

/// What the import will do, for the preview and the CLI.
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub mode: &'static str,
    pub tables: Vec<TableReport>,
    pub people: Vec<Person>,
    pub counts: PeopleCounts,
    pub handover: Vec<Handover>,
    pub refusals: Vec<PlanRefusal>,
}

// ---------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------

/// A table's own key, which the plan allocates afresh.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    /// An integer `id` drawn from this sequence.
    Int { sequence: String },
    /// A uuid `id`, minted.
    Uuid,
    /// No `id` (a junction or per-person row).
    None,
}

/// One table the import loads.
#[derive(Debug, Clone)]
pub struct TablePlan {
    pub name: String,
    pub class: Class,
    /// Rows a target already holds under the same key stay as they are
    /// (people's rows: an existing account's profile, membership).
    pub skip_conflicts: bool,
    key: Key,
    /// `(column, parent table)` for every key the rows reference, self
    /// references included.
    references: Vec<(String, String)>,
    rows: Vec<Row>,
}

impl TablePlan {
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

/// The plan. Built by [`plan_import`].
#[derive(Debug, Clone)]
pub struct ImportPlan {
    pub target: ImportTarget,
    /// Tables in load order: every parent before its children.
    tables: Vec<TablePlan>,
    people: HashMap<Uuid, usize>,
    /// The archive workspace's `settings`, which the target takes.
    pub workspace_settings: Option<Value>,
    pub report: ImportReport,
}

/// The new key of every archive row with one, by table.
#[derive(Debug, Default)]
pub struct Allocation {
    keys: HashMap<String, HashMap<String, Value>>,
}

impl Allocation {
    fn new_key(&self, table: &str, old: &Value) -> Option<&Value> {
        self.keys.get(table)?.get(&key_text(old)?)
    }
}

impl ImportPlan {
    /// Whether nothing refuses the import.
    pub fn importable(&self) -> bool {
        self.report.refusals.is_empty()
    }

    /// Tables in load order.
    pub fn tables(&self) -> &[TablePlan] {
        &self.tables
    }

    /// The people map, in archive order.
    pub fn people(&self) -> &[Person] {
        &self.report.people
    }

    /// The archive person `archive_uuid`, if the archive carries them.
    pub fn person(&self, archive_uuid: Uuid) -> Option<&Person> {
        self.people
            .get(&archive_uuid)
            .map(|i| &self.report.people[*i])
    }

    /// The `users` rows the import creates: new accounts and placeholders,
    /// under their target uuid, never platform-privileged, MFA off (the
    /// archive carries no secret).
    pub fn new_users(&self) -> Vec<Row> {
        self.report
            .people
            .iter()
            .filter(|p| p.is_new())
            .map(|p| {
                let mut row = p.user_row.clone();
                row.insert("uuid".into(), Value::String(p.target_uuid.to_string()));
                row.insert(
                    "username".into(),
                    p.username.clone().map_or(Value::Null, Value::String),
                );
                row.insert("platform_role".into(), Value::String("user".into()));
                row
            })
            .collect()
    }

    /// The primary emails the import creates, `(account, email, type)`: one
    /// per new account that has one. Placeholders get none.
    pub fn new_emails(&self) -> Vec<(Uuid, String, &'static str)> {
        self.report
            .people
            .iter()
            .filter(|p| p.outcome == Outcome::Created)
            .filter_map(|p| Some((p.target_uuid, p.email.clone()?, p.email_type)))
            .collect()
    }

    /// Draw a new key for every archive row that has one: integer ids from
    /// the table's sequence (one `nextval` per row; sequences don't roll
    /// back, so an abandoned import leaves gaps), uuid ids minted.
    pub fn allocate(&self, conn: &mut DbConnection) -> Result<Allocation, BackupError> {
        #[derive(QueryableByName)]
        struct Next {
            #[diesel(sql_type = BigInt)]
            id: i64,
        }
        if !self.importable() {
            return Err(BackupError::CorruptedBackup(
                "the import plan has refusals".to_string(),
            ));
        }
        let mut alloc = Allocation::default();
        for table in &self.tables {
            let old: Vec<String> = table
                .rows
                .iter()
                .filter_map(|r| r.get("id").and_then(key_text))
                .collect();
            let new: Vec<Value> = match &table.key {
                Key::None => continue,
                Key::Uuid => old
                    .iter()
                    .map(|_| Value::String(Uuid::now_v7().to_string()))
                    .collect(),
                Key::Int { sequence } => {
                    if old.is_empty() {
                        continue;
                    }
                    let ids: Vec<Next> = sql_query(
                        "SELECT nextval($1::regclass)::bigint AS id FROM generate_series(1, $2)",
                    )
                    .bind::<Text, _>(sequence)
                    .bind::<BigInt, _>(old.len() as i64)
                    .load(conn)
                    .map_err(BackupError::DatabaseError)?;
                    ids.into_iter().map(|n| Value::from(n.id)).collect()
                }
            };
            alloc
                .keys
                .insert(table.name.clone(), old.into_iter().zip(new).collect());
        }
        Ok(alloc)
    }

    /// `table`'s rows with its own key and every key they reference
    /// rewritten through `alloc`. User references and `workspace_id` are
    /// already the target's.
    pub fn rows(&self, table: &TablePlan, alloc: &Allocation) -> Result<Vec<Row>, BackupError> {
        let missing = |column: &str| {
            BackupError::CorruptedBackup(format!("{}.{column} has no allocated key", table.name))
        };
        let mut out = Vec::with_capacity(table.rows.len());
        for row in &table.rows {
            let mut row = row.clone();
            if table.key != Key::None {
                if let Some(old) = row.get("id") {
                    let new = alloc
                        .new_key(&table.name, old)
                        .ok_or_else(|| missing("id"))?
                        .clone();
                    row.insert("id".into(), new);
                }
            }
            for (column, parent) in &table.references {
                let Some(old) = row.get(column).filter(|v| !v.is_null()) else {
                    continue;
                };
                let new = alloc
                    .new_key(parent, old)
                    .ok_or_else(|| missing(column))?
                    .clone();
                row.insert(column.clone(), new);
            }
            out.push(row);
        }
        Ok(out)
    }
}

/// A key value as text: integers in decimal, uuids lower-case.
fn key_text(v: &Value) -> Option<String> {
    match v {
        Value::Number(n) => n.as_i64().map(|n| n.to_string()),
        Value::String(s) => Some(s.to_ascii_lowercase()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

/// A single-column integer foreign-key edge: `child_table.child_column`
/// references `parent_table.id`. Drives both the remap (rewrite the child column
/// via the parent's id map) and the topological insert order.
#[derive(Debug, Clone)]
pub struct FkEdge {
    pub child_table: String,
    pub child_column: String,
    pub parent_table: String,
}

/// Discover integer foreign-key edges (child column → `parent.id`) among
/// `tables`, from `pg_constraint`: single-column keys, and the two-column keys
/// between workspace tables, `(workspace_id, x_id)` → `(workspace_id, id)`,
/// whose edge is `x_id`. Only FKs whose referenced column is `id` are returned:
/// those are the keys a remap rewrites. User references (to `users.uuid`) go
/// through the people map instead ([`user_reference_columns`]).
pub fn discover_fk_edges(
    conn: &mut DbConnection,
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
         CROSS JOIN LATERAL unnest(con.conkey, con.confkey) AS k(child_attnum, parent_attnum) \
         JOIN pg_attribute child_col \
           ON child_col.attrelid = con.conrelid AND child_col.attnum = k.child_attnum \
         JOIN pg_attribute parent_col \
           ON parent_col.attrelid = con.confrelid AND parent_col.attnum = k.parent_attnum \
         WHERE con.contype = 'f' \
           AND con.connamespace = 'public'::regnamespace \
           AND (array_length(con.conkey, 1) = 1 \
                OR (array_length(con.conkey, 1) = 2 \
                    AND child_col.attname <> 'workspace_id' \
                    AND EXISTS ( \
                        SELECT 1 FROM unnest(con.conkey, con.confkey) AS w(child_attnum, parent_attnum) \
                        JOIN pg_attribute wc \
                          ON wc.attrelid = con.conrelid AND wc.attnum = w.child_attnum \
                        JOIN pg_attribute wp \
                          ON wp.attrelid = con.confrelid AND wp.attnum = w.parent_attnum \
                        WHERE wc.attname = 'workspace_id' AND wp.attname = 'workspace_id')))",
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

/// The `(table, column)` pairs on workspace tables whose value is a
/// `users.uuid` foreign key: every user reference the people map rewrites.
pub fn user_reference_columns(
    conn: &mut DbConnection,
) -> Result<Vec<(String, String)>, BackupError> {
    crate::services::workspace_export::user_referencing_columns(conn)
}

/// Topologically order `tables` so a parent inserts before its children, given
/// the integer-FK `edges`. Self-references (a table pointing at itself) are
/// ignored here; the plan orders a table's own rows. A cross-table cycle is
/// an error.
pub fn topological_order(tables: &[String], edges: &[FkEdge]) -> Result<Vec<String>, BackupError> {
    let set: HashSet<&str> = tables.iter().map(|s| s.as_str()).collect();
    let mut indegree: HashMap<&str, usize> = tables.iter().map(|t| (t.as_str(), 0)).collect();
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();

    for e in edges {
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
        if let Some(d) = indegree.get_mut(e.child_table.as_str()) {
            *d += 1;
        }
    }

    // Kahn's algorithm, draining in sorted order so the output is stable.
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
                if let Some(d) = indegree.get_mut(child) {
                    *d -= 1;
                    if *d == 0 {
                        ready.push(child);
                    }
                }
            }
            ready.sort_unstable();
            queue.extend(ready);
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

/// A unique index on a carried table.
#[derive(Debug, Clone)]
struct UniqueKey {
    table: String,
    columns: Vec<String>,
    predicate: Option<String>,
}

impl UniqueKey {
    /// Unique per workspace rather than across the instance.
    fn scoped(&self) -> bool {
        self.columns.iter().any(|c| c == "workspace_id")
    }
}

/// What the plan reads from the target's catalog.
struct Catalog {
    edges: Vec<FkEdge>,
    user_columns: HashMap<String, Vec<String>>,
    keys: HashMap<String, Key>,
    unique_keys: Vec<UniqueKey>,
    /// `encrypted_*` columns, and the key-id columns beside them.
    sealed_columns: HashMap<String, Vec<String>>,
}

impl Catalog {
    fn read(conn: &mut DbConnection, tables: &[String]) -> Result<Self, BackupError> {
        let set: HashSet<String> = tables.iter().cloned().collect();
        let edges = discover_fk_edges(conn, &set)?;
        let mut user_columns: HashMap<String, Vec<String>> = HashMap::new();
        for (table, column) in user_reference_columns(conn)? {
            if set.contains(&table) {
                user_columns.entry(table).or_default().push(column);
            }
        }
        Ok(Self {
            edges,
            user_columns,
            keys: table_keys(conn, tables)?,
            unique_keys: unique_keys(conn, tables)?,
            sealed_columns: sealed_columns(conn, tables)?,
        })
    }
}

fn table_keys(
    conn: &mut DbConnection,
    tables: &[String],
) -> Result<HashMap<String, Key>, BackupError> {
    #[derive(QueryableByName)]
    struct KeyRow {
        #[diesel(sql_type = Text)]
        table_name: String,
        #[diesel(sql_type = Nullable<Text>)]
        id_type: Option<String>,
        #[diesel(sql_type = Nullable<Text>)]
        sequence: Option<String>,
    }
    let rows: Vec<KeyRow> = sql_query(
        "SELECT c.relname::text AS table_name, \
                format_type(a.atttypid, a.atttypmod) AS id_type, \
                CASE WHEN a.attnum IS NULL THEN NULL \
                     ELSE pg_get_serial_sequence('public.' || quote_ident(c.relname), 'id') \
                END AS sequence \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public' \
         LEFT JOIN pg_attribute a \
           ON a.attrelid = c.oid AND a.attname = 'id' AND a.attnum > 0 AND NOT a.attisdropped \
         WHERE c.relkind IN ('r', 'p') AND c.relname = ANY($1)",
    )
    .bind::<Array<Text>, _>(tables)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    let mut keys = HashMap::new();
    for row in rows {
        let key = match (row.id_type.as_deref(), row.sequence) {
            (None, _) => Key::None,
            (Some("uuid"), _) => Key::Uuid,
            (Some("integer" | "bigint" | "smallint"), Some(sequence)) => Key::Int { sequence },
            (Some(other), _) => {
                return Err(BackupError::CorruptedBackup(format!(
                    "{}.id ({other}) has no sequence to allocate from",
                    row.table_name
                )))
            }
        };
        keys.insert(row.table_name, key);
    }
    Ok(keys)
}

fn unique_keys(conn: &mut DbConnection, tables: &[String]) -> Result<Vec<UniqueKey>, BackupError> {
    #[derive(QueryableByName)]
    struct IndexRow {
        #[diesel(sql_type = Text)]
        table_name: String,
        #[diesel(sql_type = Array<Text>)]
        columns: Vec<String>,
        #[diesel(sql_type = Nullable<Text>)]
        predicate: Option<String>,
    }
    let rows: Vec<IndexRow> = sql_query(
        "SELECT c.relname::text AS table_name, \
                array_agg(a.attname::text ORDER BY k.ord) AS columns, \
                pg_get_expr(x.indpred, x.indrelid) AS predicate \
         FROM pg_index x \
         JOIN pg_class c ON c.oid = x.indrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public' \
         CROSS JOIN LATERAL unnest(x.indkey::int2[]) WITH ORDINALITY AS k(attnum, ord) \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum \
         WHERE x.indisunique AND x.indexprs IS NULL AND k.ord <= x.indnkeyatts \
           AND c.relname = ANY($1) \
         GROUP BY x.indexrelid, c.relname, x.indpred, x.indrelid \
         ORDER BY c.relname, x.indexrelid",
    )
    .bind::<Array<Text>, _>(tables)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    Ok(rows
        .into_iter()
        .map(|r| UniqueKey {
            table: r.table_name,
            columns: r.columns,
            predicate: r.predicate,
        })
        .collect())
}

fn sealed_columns(
    conn: &mut DbConnection,
    tables: &[String],
) -> Result<HashMap<String, Vec<String>>, BackupError> {
    #[derive(QueryableByName)]
    struct ColumnRow {
        #[diesel(sql_type = Text)]
        table_name: String,
        #[diesel(sql_type = Text)]
        column_name: String,
    }
    // Every `encrypted_*` column, and the key-id columns (`*kek_id`) on the
    // same tables, which name a key in the source's keyring.
    let rows: Vec<ColumnRow> = sql_query(
        "SELECT c.relname::text AS table_name, a.attname::text AS column_name \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public' \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
         WHERE c.relname = ANY($1) \
           AND (a.attname LIKE 'encrypted\\_%' OR a.attname LIKE '%kek\\_id') \
           AND EXISTS (SELECT 1 FROM pg_attribute e WHERE e.attrelid = c.oid \
                       AND e.attname LIKE 'encrypted\\_%' AND NOT e.attisdropped) \
         ORDER BY c.relname, a.attnum",
    )
    .bind::<Array<Text>, _>(tables)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for row in rows {
        out.entry(row.table_name).or_default().push(row.column_name);
    }
    Ok(out)
}

/// Whether `column` holds a sealed value (not a key id).
fn is_sealed_value(column: &str) -> bool {
    column.starts_with("encrypted_") && !column.ends_with("kek_id")
}

// ---------------------------------------------------------------------------
// Building the plan
// ---------------------------------------------------------------------------

/// The archive's tables that aren't workspace tables here (nor the people
/// and workspace entries). An import refuses an archive that names any,
/// before it writes: the archive is untrusted, and the load runs with RLS
/// bypassed and audit capture off.
pub fn tables_outside_workspace(
    conn: &mut DbConnection,
    contents: &ArchiveContents,
) -> Result<Vec<String>, BackupError> {
    let workspace: HashSet<String> = crate::services::workspace_export::all_workspace_tables(conn)?
        .into_iter()
        .collect();
    let mut outside: Vec<String> = contents
        .tables
        .keys()
        .filter(|t| !is_people_or_workspace(t))
        .filter(|t| !workspace.contains(*t))
        .cloned()
        .collect();
    outside.sort_unstable();
    Ok(outside)
}

fn class_name(class: Class) -> &'static str {
    match class {
        Class::Move => "move",
        Class::Replace => "replace",
        Class::Merge => "merge",
        Class::Never => "never",
        Class::Excluded => "excluded",
    }
}

/// Plan importing `contents` into `target`. Reads the target; writes
/// nothing. Run it on a BYPASSRLS connection: uniqueness and email holders
/// are instance-wide.
pub fn plan_import(
    conn: &mut DbConnection,
    contents: &ArchiveContents,
    target: &ImportTarget,
    options: PlanOptions,
) -> Result<ImportPlan, BackupError> {
    let mut refusals: Vec<PlanRefusal> = Vec::new();
    let mut table_reports: BTreeMap<String, TableReport> = BTreeMap::new();

    let outside = tables_outside_workspace(conn, contents)?;
    if !outside.is_empty() {
        refusals.push(PlanRefusal::NotWorkspaceTables {
            tables: outside.clone(),
        });
    }

    // Which archive tables load: the carried classes. The rest are reported
    // and skipped (an archive from before the policy may carry them).
    let mut carried: Vec<String> = Vec::new();
    for (name, rows) in &contents.tables {
        if is_people_or_workspace(name) || outside.contains(name) {
            continue;
        }
        let class = policy::table(name).map_or(Class::Move, |t| t.class);
        table_reports.insert(
            name.clone(),
            TableReport {
                table: name.clone(),
                class: class_name(class),
                in_archive: rows.len(),
                planned: 0,
                dropped: BTreeMap::new(),
                cleared: BTreeMap::new(),
            },
        );
        if class.carried() {
            carried.push(name.clone());
        }
    }
    carried.sort_unstable();
    let catalog = Catalog::read(conn, &carried)?;

    let people = build_people(conn, contents, target, &mut refusals)?;
    let by_archive: HashMap<Uuid, usize> = people
        .iter()
        .enumerate()
        .map(|(i, p)| (p.archive_uuid, i))
        .collect();

    let mut handover = HandoverTally::default();
    let mut tables: HashMap<String, TablePlan> = HashMap::new();
    for name in &carried {
        let class = policy::table(name).map_or(Class::Move, |t| t.class);
        let report = table_reports
            .get_mut(name)
            .ok_or_else(|| BackupError::CorruptedBackup(format!("{name}: no report")))?;
        let user_columns = catalog.user_columns.get(name).cloned().unwrap_or_default();
        let mut rows: Vec<Row> = Vec::new();
        let mut unknown: BTreeMap<String, usize> = BTreeMap::new();
        for value in contents.tables.get(name).into_iter().flatten() {
            let Some(row) = value.as_object() else {
                refusals.push(PlanRefusal::BadRow {
                    table: name.clone(),
                    reason: "not an object".into(),
                });
                break;
            };
            let mut row = row.clone();
            if let Some(reason) = person_rule(name, &row, &people, &by_archive, target) {
                *report.dropped.entry(reason).or_default() += 1;
                continue;
            }
            if let Some(reason) = handover.apply(name, &mut row, &catalog) {
                *report.dropped.entry(reason).or_default() += 1;
                continue;
            }
            if name == "workspace_members" {
                adjust_membership(&mut row, &people, &by_archive);
            }
            // Every user reference becomes the target account's.
            let mut bad = None;
            for column in &user_columns {
                let Some(old) = row.get(column).filter(|v| !v.is_null()) else {
                    continue;
                };
                let target_uuid = old
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .and_then(|u| by_archive.get(&u))
                    .map(|i| people[*i].target_uuid);
                match target_uuid {
                    Some(u) => {
                        row.insert(column.clone(), Value::String(u.to_string()));
                    }
                    None => bad = Some(column.clone()),
                }
            }
            if let Some(column) = bad {
                *unknown.entry(column).or_default() += 1;
                continue;
            }
            for (t, column) in USER_REFERENCES_WITHOUT_FK {
                if t == name {
                    let cleared = remap_loose(&mut row, column, &people, &by_archive);
                    if cleared > 0 {
                        *report.cleared.entry((*column).to_string()).or_default() += cleared;
                    }
                }
            }
            if row.contains_key("workspace_id") {
                row.insert("workspace_id".into(), Value::from(target.workspace_id));
            }
            if options.regenerate_row_uuids && row.get("uuid").is_some_and(Value::is_string) {
                row.insert("uuid".into(), Value::String(Uuid::now_v7().to_string()));
            }
            rows.push(row);
        }
        for (column, n) in unknown {
            refusals.push(PlanRefusal::UnknownPerson {
                table: name.clone(),
                column,
                rows: n,
            });
        }
        let key = catalog.keys.get(name).cloned().unwrap_or(Key::None);
        let references: Vec<(String, String)> = catalog
            .edges
            .iter()
            .filter(|e| &e.child_table == name)
            .map(|e| (e.child_column.clone(), e.parent_table.clone()))
            .collect();
        tables.insert(
            name.clone(),
            TablePlan {
                name: name.clone(),
                class,
                skip_conflicts: class == Class::Merge,
                key,
                references,
                rows,
            },
        );
    }

    // Two archive people on one account (both of a runner's addresses, say)
    // can make two rows of one person-keyed row; the first stays.
    for key in &catalog.unique_keys {
        let Some(table) = tables.get_mut(&key.table) else {
            continue;
        };
        let users = catalog.user_columns.get(&key.table);
        let person_keyed = key
            .columns
            .iter()
            .any(|c| users.is_some_and(|u| u.contains(c)));
        if !person_keyed || key.predicate.is_some() {
            continue;
        }
        let mut seen: HashSet<String> = HashSet::new();
        let before = table.rows.len();
        table.rows.retain(|row| match tuple(row, &key.columns) {
            Some(t) => seen.insert(t),
            None => true,
        });
        let dropped = before - table.rows.len();
        if dropped > 0 {
            if let Some(report) = table_reports.get_mut(&key.table) {
                *report.dropped.entry("same_account_twice").or_default() += dropped;
            }
        }
    }

    check_references(&mut tables, &mut refusals);
    check_collisions(conn, &tables, &catalog, &mut refusals)?;

    let names: Vec<String> = tables.keys().cloned().collect();
    let order = match topological_order(&names, &catalog.edges) {
        Ok(order) => order,
        Err(_) => {
            refusals.push(PlanRefusal::ReferenceCycle {
                table: "(several)".into(),
            });
            let mut names = names;
            names.sort_unstable();
            names
        }
    };
    let ordered: Vec<TablePlan> = order
        .into_iter()
        .filter_map(|name| tables.remove(&name))
        .collect();
    for table in &ordered {
        if let Some(report) = table_reports.get_mut(&table.name) {
            report.planned = table.rows.len();
        }
    }

    let workspace_settings = contents
        .tables
        .get("workspaces")
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("settings"))
        .cloned();

    let mut counts = PeopleCounts::default();
    for p in &people {
        match p.outcome {
            Outcome::MergedIntoRunner => counts.merged_into_runner += 1,
            Outcome::Mapped => counts.mapped += 1,
            Outcome::Created => counts.created += 1,
            Outcome::Placeholder => counts.placeholders += 1,
        }
        if p.placeholder == Some(PlaceholderReason::RequesterUnlinked) {
            counts.requesters_unlinked += 1;
        }
        counts.usernames_dropped += usize::from(p.username_dropped);
        counts.uuids_reminted += usize::from(p.uuid_reminted);
    }

    Ok(ImportPlan {
        target: target.clone(),
        tables: ordered,
        people: by_archive,
        workspace_settings,
        report: ImportReport {
            mode: match target.mode {
                DeploymentMode::SelfHosted => "self_hosted",
                DeploymentMode::Hosted => "hosted",
            },
            tables: table_reports.into_values().collect(),
            people,
            counts,
            handover: handover.into_items(),
            refusals,
        },
    })
}

/// The JSON of `columns` in `row`, or `None` when any is null or absent (a
/// unique index never matches a NULL).
fn tuple(row: &Row, columns: &[String]) -> Option<String> {
    let mut parts = Vec::with_capacity(columns.len());
    for c in columns {
        match row.get(c) {
            None | Some(Value::Null) => return None,
            Some(v) => parts.push(v.to_string()),
        }
    }
    Some(parts.join("\u{1f}"))
}

/// The people rule for one row of a person-keyed table, by its archive
/// `user_uuid`: why the row stays out, or `None` to load it.
fn person_rule(
    table: &str,
    row: &Row,
    people: &[Person],
    by_archive: &HashMap<Uuid, usize>,
    target: &ImportTarget,
) -> Option<&'static str> {
    if !matches!(
        table,
        "workspace_members" | "notification_preferences" | "user_auth_identities"
    ) {
        return None;
    }
    let person = row
        .get("user_uuid")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .and_then(|u| by_archive.get(&u))
        .map(|i| &people[*i])?;
    match table {
        // Placeholders join through their seat; hosted staff seats are the
        // control plane's to give.
        "workspace_members" => {
            if person.outcome == Outcome::Placeholder {
                return Some("placeholder");
            }
            if target.mode == DeploymentMode::Hosted && person.outcome == Outcome::MergedIntoRunner
            {
                return Some("runner_seat");
            }
            let role = row.get("role").and_then(Value::as_str).unwrap_or_default();
            if target.mode == DeploymentMode::Hosted && STAFF_ROLES.contains(&role) {
                return Some("staff_seat");
            }
            None
        }
        // Unique per account, not per workspace (F5): an existing account
        // keeps its own.
        "notification_preferences" => (!person.is_new()).then_some("existing_account"),
        // A sign-in identity on an existing account would link it to the
        // archive's directory or IdP; on a placeholder, sign in before the
        // seat is given.
        "user_auth_identities" => match person.outcome {
            Outcome::Created => None,
            Outcome::Placeholder => Some("placeholder"),
            _ => Some("existing_account"),
        },
        _ => None,
    }
}

/// Rewrite a user reference without a foreign key through the people map:
/// a uuid, or an array of them. Returns how many values named nobody the
/// archive carries, which are cleared.
fn remap_loose(
    row: &mut Row,
    column: &str,
    people: &[Person],
    by_archive: &HashMap<Uuid, usize>,
) -> usize {
    let map = |v: &Value| {
        v.as_str()
            .and_then(|s| Uuid::parse_str(s).ok())
            .and_then(|u| by_archive.get(&u))
            .map(|i| Value::String(people[*i].target_uuid.to_string()))
    };
    let mut cleared = 0;
    let new = match row.get(column) {
        None | Some(Value::Null) => return 0,
        Some(Value::Array(items)) => Value::Array(
            items
                .iter()
                .filter_map(|v| {
                    let mapped = map(v);
                    cleared += usize::from(mapped.is_none());
                    mapped
                })
                .collect(),
        ),
        Some(v) => map(v).unwrap_or_else(|| {
            cleared += 1;
            Value::Null
        }),
    };
    row.insert(column.to_string(), new);
    cleared
}

/// A membership row takes the person's role on the target
/// ([`Person::role_on_target`]): an existing membership's, else the archive's
/// with an owner other than the runner as admin when the target has an owner
/// (N5).
fn adjust_membership(row: &mut Row, people: &[Person], by_archive: &HashMap<Uuid, usize>) {
    let role = row
        .get("user_uuid")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .and_then(|u| by_archive.get(&u))
        .and_then(|i| people[*i].role_on_target.clone());
    if let Some(role) = role {
        row.insert("role".into(), Value::String(role));
    }
}

/// Every reference into another carried table names a row the plan loads,
/// and a table's rows that reference each other load parents first.
fn check_references(tables: &mut HashMap<String, TablePlan>, refusals: &mut Vec<PlanRefusal>) {
    let present: HashMap<String, HashSet<String>> = tables
        .iter()
        .filter(|(_, t)| t.key != Key::None)
        .map(|(name, t)| {
            (
                name.clone(),
                t.rows
                    .iter()
                    .filter_map(|r| r.get("id").and_then(key_text))
                    .collect(),
            )
        })
        .collect();
    let mut names: Vec<String> = tables.keys().cloned().collect();
    names.sort_unstable();
    for name in names {
        let Some(table) = tables.get_mut(&name) else {
            continue;
        };
        if table.key != Key::None
            && table
                .rows
                .iter()
                .any(|r| r.get("id").and_then(key_text).is_none())
        {
            refusals.push(PlanRefusal::BadRow {
                table: name.clone(),
                reason: "a row has no id".into(),
            });
            continue;
        }
        for (column, parent) in &table.references {
            let keys = present.get(parent);
            let missing = table
                .rows
                .iter()
                .filter_map(|r| r.get(column).filter(|v| !v.is_null()))
                .filter(|v| !key_text(v).is_some_and(|k| keys.is_some_and(|s| s.contains(&k))))
                .count();
            if missing > 0 {
                refusals.push(PlanRefusal::MissingParent {
                    table: name.clone(),
                    column: column.clone(),
                    parent: parent.clone(),
                    rows: missing,
                });
            }
        }
        let own: Vec<String> = table
            .references
            .iter()
            .filter(|(_, parent)| *parent == name)
            .map(|(column, _)| column.clone())
            .collect();
        if !own.is_empty() && !order_own_rows(&mut table.rows, &own) {
            refusals.push(PlanRefusal::ReferenceCycle {
                table: name.clone(),
            });
        }
    }
}

/// Order rows so each comes after the rows of its own table it references.
/// False on a loop.
fn order_own_rows(rows: &mut Vec<Row>, columns: &[String]) -> bool {
    let ids: Vec<Option<String>> = rows
        .iter()
        .map(|r| r.get("id").and_then(key_text))
        .collect();
    let index: HashMap<&str, usize> = ids
        .iter()
        .enumerate()
        .filter_map(|(i, id)| id.as_deref().map(|id| (id, i)))
        .collect();
    let mut placed = vec![false; rows.len()];
    let mut order: Vec<usize> = Vec::with_capacity(rows.len());
    loop {
        let before = order.len();
        for i in 0..rows.len() {
            if placed[i] {
                continue;
            }
            let ready = columns.iter().all(|c| {
                rows[i]
                    .get(c)
                    .and_then(key_text)
                    .and_then(|k| index.get(k.as_str()))
                    .is_none_or(|p| placed[*p] || *p == i)
            });
            if ready {
                placed[i] = true;
                order.push(i);
            }
        }
        if order.len() == rows.len() {
            break;
        }
        if order.len() == before {
            return false;
        }
    }
    let mut taken: Vec<Option<Row>> = std::mem::take(rows).into_iter().map(Some).collect();
    *rows = order.into_iter().filter_map(|i| taken[i].take()).collect();
    true
}

/// Refuse rows whose instance-unique values the target already holds or
/// the archive repeats. Keys the plan allocates afresh can't collide; the
/// rest (row uuids, message ids, people's per-account rows) are checked.
fn check_collisions(
    conn: &mut DbConnection,
    tables: &HashMap<String, TablePlan>,
    catalog: &Catalog,
    refusals: &mut Vec<PlanRefusal>,
) -> Result<(), BackupError> {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = BigInt)]
        n: i64,
    }
    for key in &catalog.unique_keys {
        if key.scoped() {
            continue;
        }
        let Some(table) = tables.get(&key.table) else {
            continue;
        };
        if table.rows.is_empty() {
            continue;
        }
        let fresh = |c: &String| {
            (c == "id" && table.key != Key::None) || table.references.iter().any(|(r, _)| r == c)
        };
        if key.columns.iter().any(fresh) {
            continue;
        }
        let mut seen: HashSet<String> = HashSet::new();
        let mut repeated = 0usize;
        if key.predicate.is_none() {
            for row in &table.rows {
                if let Some(t) = tuple(row, &key.columns) {
                    if !seen.insert(t) {
                        repeated += 1;
                    }
                }
            }
        }
        let matches = key
            .columns
            .iter()
            .map(|c| format!("b.\"{c}\" = a.\"{c}\""))
            .collect::<Vec<_>>()
            .join(" AND ");
        let predicate = key.predicate.as_deref().unwrap_or("true");
        // Unqualified names in the predicate are the archive row's outside
        // the EXISTS and the existing row's inside it.
        let query = format!(
            "SELECT count(*)::bigint AS n \
             FROM jsonb_populate_recordset(NULL::public.\"{t}\", $1::jsonb) a \
             WHERE ({predicate}) AND EXISTS ( \
               SELECT 1 FROM public.\"{t}\" b WHERE {matches} AND ({predicate}))",
            t = key.table
        );
        let payload = serde_json::to_string(&table.rows).map_err(BackupError::JsonError)?;
        let held: Count = sql_query(query)
            .bind::<Text, _>(payload)
            .get_result(conn)
            .map_err(BackupError::DatabaseError)?;
        let rows = held.n as usize + repeated;
        if rows > 0 {
            refusals.push(PlanRefusal::Collision {
                table: key.table.clone(),
                columns: key.columns.clone(),
                rows,
            });
        }
    }
    Ok(())
}

/// What the handover items did, for the report.
#[derive(Default)]
struct HandoverTally {
    sealed: BTreeMap<(String, String), usize>,
    regenerated: BTreeMap<(String, String), usize>,
    plugin_secrets: usize,
    switched_off: BTreeMap<(String, String), usize>,
}

impl HandoverTally {
    /// Apply the handover items to one row: why it stays out, or `None`.
    fn apply(&mut self, table: &str, row: &mut Row, catalog: &Catalog) -> Option<&'static str> {
        if table == PLUGIN_SECRETS.0 && row.get(PLUGIN_SECRETS.1) == Some(&Value::Bool(true)) {
            self.plugin_secrets += 1;
            return Some("plugin_secret");
        }
        for column in catalog.sealed_columns.get(table).into_iter().flatten() {
            let held = row.get(column).is_some_and(|v| !v.is_null());
            if held && is_sealed_value(column) {
                *self
                    .sealed
                    .entry((table.to_string(), column.clone()))
                    .or_default() += 1;
            }
            if row.contains_key(column) {
                row.insert(column.clone(), Value::Null);
            }
        }
        for (t, column) in REGENERATED {
            if *t == table && row.get(*column).is_some_and(|v| !v.is_null()) {
                row.insert(
                    (*column).to_string(),
                    Value::String(Uuid::new_v4().to_string()),
                );
                *self
                    .regenerated
                    .entry((table.to_string(), (*column).to_string()))
                    .or_default() += 1;
            }
        }
        for (t, column, on, off) in SWITCHED_OFF {
            if *t != table {
                continue;
            }
            let (Ok(on), Ok(off)) = (
                serde_json::from_str::<Value>(on),
                serde_json::from_str::<Value>(off),
            ) else {
                continue;
            };
            if row.get(*column) == Some(&on) {
                row.insert((*column).to_string(), off);
                *self
                    .switched_off
                    .entry((table.to_string(), (*column).to_string()))
                    .or_default() += 1;
            }
        }
        None
    }

    fn into_items(self) -> Vec<Handover> {
        let mut items: Vec<Handover> = Vec::new();
        for ((table, column), rows) in self.sealed {
            items.push(Handover::EncryptedSettingDropped {
                table,
                column,
                rows,
            });
        }
        for ((table, column), rows) in self.regenerated {
            items.push(Handover::TokensRegenerated {
                table,
                column,
                rows,
            });
        }
        if self.plugin_secrets > 0 {
            items.push(Handover::PluginSecretsDropped {
                rows: self.plugin_secrets,
            });
        }
        for ((table, column), rows) in self.switched_off {
            items.push(Handover::SwitchedOff {
                table,
                column,
                rows,
            });
        }
        items
    }
}

// ---------------------------------------------------------------------------
// People map
// ---------------------------------------------------------------------------

/// An account on the target that holds an archive person's email.
struct Holder {
    user_uuid: Uuid,
    /// The address is verified or not the account's primary, the account is
    /// privileged (platform role, or staff in any workspace), or it signs in
    /// through an IdP: a hosted requester's history doesn't attach to it.
    guarded: bool,
}

/// Map every archive person (users.json) to the account their rows land on.
fn build_people(
    conn: &mut DbConnection,
    contents: &ArchiveContents,
    target: &ImportTarget,
    refusals: &mut Vec<PlanRefusal>,
) -> Result<Vec<Person>, BackupError> {
    struct Archived {
        uuid: Uuid,
        row: Row,
        email: Option<String>,
        email_type: &'static str,
        role: Option<String>,
        removed: bool,
    }

    // Archive people, their primary emails and their roles.
    let mut archived: Vec<Archived> = Vec::new();
    let mut seen: HashSet<Uuid> = HashSet::new();
    for value in contents.tables.get("users").into_iter().flatten() {
        let Some(uuid) = value
            .get("uuid")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
        else {
            refusals.push(PlanRefusal::BadRow {
                table: "users".into(),
                reason: "a person has no valid uuid".into(),
            });
            continue;
        };
        if !seen.insert(uuid) {
            refusals.push(PlanRefusal::DuplicatePerson {
                uuid: uuid.to_string(),
            });
            continue;
        }
        let mut row = Row::new();
        for column in policy::ARCHIVE_USER_COLUMNS {
            if let Some(v) = value.get(*column) {
                row.insert((*column).to_string(), v.clone());
            }
        }
        archived.push(Archived {
            uuid,
            row,
            email: None,
            email_type: "personal",
            role: None,
            removed: false,
        });
    }
    let index: HashMap<Uuid, usize> = archived
        .iter()
        .enumerate()
        .map(|(i, a)| (a.uuid, i))
        .collect();
    let mut emails_seen: HashSet<String> = HashSet::new();
    for value in contents.tables.get("user_emails").into_iter().flatten() {
        if value.get("is_primary") == Some(&Value::Bool(false)) {
            continue;
        }
        let Some(i) = value
            .get("user_uuid")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .and_then(|u| index.get(&u))
        else {
            continue;
        };
        let email = value
            .get("email")
            .and_then(Value::as_str)
            .map(crate::utils::normalize_email)
            .filter(|e| !e.is_empty());
        let (Some(email), None) = (email, &archived[*i].email) else {
            continue;
        };
        if !emails_seen.insert(email.clone()) {
            refusals.push(PlanRefusal::DuplicateEmail { email });
            continue;
        }
        archived[*i].email_type = if value.get("email_type").and_then(Value::as_str) == Some("work")
        {
            "work"
        } else {
            "personal"
        };
        archived[*i].email = Some(email);
    }
    for value in contents
        .tables
        .get("workspace_members")
        .into_iter()
        .flatten()
    {
        let Some(i) = value
            .get("user_uuid")
            .and_then(Value::as_str)
            .and_then(|s| Uuid::parse_str(s).ok())
            .and_then(|u| index.get(&u))
        else {
            continue;
        };
        let a = &mut archived[*i];
        if a.role.is_none() {
            a.role = value
                .get("role")
                .and_then(Value::as_str)
                .map(str::to_string);
            a.removed = value.get("removed_at").is_some_and(|v| !v.is_null());
        }
    }

    // The target: who holds each email, the runner's emails, taken uuids and
    // usernames, and current memberships.
    let emails: Vec<String> = archived.iter().filter_map(|a| a.email.clone()).collect();
    let holders = email_holders(conn, &emails)?;
    let runner_emails = runner_emails(conn, target)?;
    let runner = target.runner.user();
    let archive_uuids: Vec<Uuid> = archived.iter().map(|a| a.uuid).collect();
    let taken_uuids = taken_uuids(conn, &archive_uuids)?;
    let usernames: Vec<String> = archived
        .iter()
        .filter_map(|a| a.row.get("username").and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    let mut taken_usernames = taken_usernames(conn, &usernames)?;
    let (target_has_owner, mut current_roles) = target_memberships(conn, target.workspace_id)?;

    let mut used_uuids: HashSet<Uuid> = HashSet::new();
    let mut people = Vec::with_capacity(archived.len());
    for a in archived {
        let staff = a.role.as_deref().is_some_and(|r| STAFF_ROLES.contains(&r));
        let disabled = a.removed || a.row.get("deleted_at").is_some_and(|v| !v.is_null());
        let holder = a.email.as_ref().and_then(|e| holders.get(e));
        let runs_it = a.email.as_ref().is_some_and(|e| runner_emails.contains(e));

        // An archive uuid is never looked up: the existing account comes
        // from the email alone.
        let (outcome, placeholder, existing) = match (runner, runs_it) {
            (Some(runner), true) => (Outcome::MergedIntoRunner, None, Some(runner)),
            _ => match target.mode {
                DeploymentMode::SelfHosted => match holder {
                    Some(h) => (Outcome::Mapped, None, Some(h.user_uuid)),
                    None => (Outcome::Created, None, None),
                },
                DeploymentMode::Hosted => match (&a.email, holder) {
                    _ if staff => (Outcome::Placeholder, Some(PlaceholderReason::Staff), None),
                    (None, _) => (Outcome::Placeholder, Some(PlaceholderReason::NoEmail), None),
                    (Some(_), None) => (Outcome::Created, None, None),
                    (Some(_), Some(h)) if !h.guarded => (Outcome::Mapped, None, Some(h.user_uuid)),
                    (Some(_), Some(_)) => (
                        Outcome::Placeholder,
                        Some(PlaceholderReason::RequesterUnlinked),
                        None,
                    ),
                },
            },
        };

        let (target_uuid, uuid_reminted) = match existing {
            Some(u) => (u, false),
            None if !taken_uuids.contains(&a.uuid) && !used_uuids.contains(&a.uuid) => {
                (a.uuid, false)
            }
            None => (Uuid::now_v7(), true),
        };
        used_uuids.insert(target_uuid);

        let archive_username = a
            .row
            .get("username")
            .and_then(Value::as_str)
            .map(str::to_string);
        let (username, username_dropped) = match (existing, archive_username) {
            (None, Some(name)) if taken_usernames.insert(name.clone()) => (Some(name), false),
            (None, Some(_)) => (None, true),
            _ => (None, false),
        };

        // Role on the target: an existing membership stands; otherwise the
        // archive's, with an owner other than the runner as admin when the
        // target has an owner. Placeholders and hosted staff have none.
        let archive_role = a.role.clone();
        let role_on_target = match existing.and_then(|u| current_roles.remove(&u)) {
            Some(current) => Some(current),
            None if outcome == Outcome::Placeholder => None,
            None if target.mode == DeploymentMode::Hosted && staff => None,
            None => archive_role.clone().map(|role| {
                if role == "owner" && target_has_owner && outcome != Outcome::MergedIntoRunner {
                    "admin".to_string()
                } else {
                    role
                }
            }),
        };
        if let (Some(u), Some(role)) = (existing, &role_on_target) {
            current_roles.insert(u, role.clone());
        }

        let name = a
            .row
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        people.push(Person {
            archive_uuid: a.uuid,
            target_uuid,
            name,
            email: a.email,
            archive_role,
            staff,
            disabled,
            outcome,
            placeholder,
            role_on_target,
            username_dropped,
            uuid_reminted,
            username,
            email_type: a.email_type,
            user_row: a.row,
        });
    }
    Ok(people)
}

fn email_holders(
    conn: &mut DbConnection,
    emails: &[String],
) -> Result<HashMap<String, Holder>, BackupError> {
    #[derive(QueryableByName)]
    struct HolderRow {
        #[diesel(sql_type = Text)]
        email: String,
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        user_uuid: Uuid,
        #[diesel(sql_type = Bool)]
        guarded: bool,
    }
    if emails.is_empty() {
        return Ok(HashMap::new());
    }
    // Any row holds an address (emails are unique across accounts), primary
    // or not.
    let rows: Vec<HolderRow> = sql_query(
        "SELECT lower(ue.email) AS email, ue.user_uuid, \
                (ue.is_verified \
                 OR NOT ue.is_primary \
                 OR u.platform_role <> 'user' \
                 OR EXISTS (SELECT 1 FROM public.workspace_members m \
                            WHERE m.user_uuid = ue.user_uuid AND m.removed_at IS NULL \
                              AND m.role IN ('owner', 'admin', 'agent')) \
                 OR EXISTS (SELECT 1 FROM public.user_auth_identities i \
                            WHERE i.user_uuid = ue.user_uuid AND i.workspace_id IS NULL \
                              AND i.provider_type <> 'local')) AS guarded \
         FROM public.user_emails ue JOIN public.users u ON u.uuid = ue.user_uuid \
         WHERE lower(ue.email) = ANY($1)",
    )
    .bind::<Array<Text>, _>(emails)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    Ok(rows
        .into_iter()
        .map(|r| {
            (
                r.email,
                Holder {
                    user_uuid: r.user_uuid,
                    guarded: r.guarded,
                },
            )
        })
        .collect())
}

/// The emails whose archive person merges into the runner, normalised.
fn runner_emails(
    conn: &mut DbConnection,
    target: &ImportTarget,
) -> Result<HashSet<String>, BackupError> {
    #[derive(QueryableByName)]
    struct Email {
        #[diesel(sql_type = Text)]
        email: String,
    }
    let primary_of = |conn: &mut DbConnection, user: Uuid| {
        sql_query(
            "SELECT lower(email) AS email FROM public.user_emails \
             WHERE user_uuid = $1 AND is_primary",
        )
        .bind::<diesel::sql_types::Uuid, _>(user)
        .load::<Email>(conn)
        .map(|rows| rows.into_iter().map(|r| r.email).collect::<HashSet<_>>())
        .map_err(BackupError::DatabaseError)
    };
    match &target.runner {
        Runner::SelfHostedOwner { user } => primary_of(conn, *user),
        Runner::HostedOwner {
            verified_emails, ..
        } => Ok(verified_emails
            .iter()
            .map(|e| crate::utils::normalize_email(e))
            .collect()),
        Runner::Operator { user: Some(user) } if target.mode == DeploymentMode::SelfHosted => {
            primary_of(conn, *user)
        }
        Runner::Operator { .. } => Ok(HashSet::new()),
    }
}

fn taken_uuids(conn: &mut DbConnection, uuids: &[Uuid]) -> Result<HashSet<Uuid>, BackupError> {
    #[derive(QueryableByName)]
    struct Taken {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        uuid: Uuid,
    }
    if uuids.is_empty() {
        return Ok(HashSet::new());
    }
    Ok(
        sql_query("SELECT uuid FROM public.users WHERE uuid = ANY($1)")
            .bind::<Array<diesel::sql_types::Uuid>, _>(uuids)
            .load::<Taken>(conn)
            .map_err(BackupError::DatabaseError)?
            .into_iter()
            .map(|t| t.uuid)
            .collect(),
    )
}

fn taken_usernames(
    conn: &mut DbConnection,
    usernames: &[String],
) -> Result<HashSet<String>, BackupError> {
    #[derive(QueryableByName)]
    struct Taken {
        #[diesel(sql_type = Text)]
        username: String,
    }
    if usernames.is_empty() {
        return Ok(HashSet::new());
    }
    Ok(
        sql_query("SELECT username::text AS username FROM public.users WHERE username = ANY($1)")
            .bind::<Array<Text>, _>(usernames)
            .load::<Taken>(conn)
            .map_err(BackupError::DatabaseError)?
            .into_iter()
            .map(|t| t.username)
            .collect(),
    )
}

/// Whether the target workspace has an active owner, and every active
/// member's role there.
fn target_memberships(
    conn: &mut DbConnection,
    workspace_id: i32,
) -> Result<(bool, HashMap<Uuid, String>), BackupError> {
    #[derive(QueryableByName)]
    struct Member {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        user_uuid: Uuid,
        #[diesel(sql_type = Text)]
        role: String,
    }
    let members: Vec<Member> = sql_query(
        "SELECT user_uuid, role::text AS role FROM public.workspace_members \
         WHERE workspace_id = $1 AND removed_at IS NULL",
    )
    .bind::<Integer, _>(workspace_id)
    .load(conn)
    .map_err(BackupError::DatabaseError)?;
    let has_owner = members.iter().any(|m| m.role == "owner");
    Ok((
        has_owner,
        members.into_iter().map(|m| (m.user_uuid, m.role)).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::workspace_export::WorkspaceExportManifest;
    use crate::sync::actor::ActorContext;
    use crate::sync::session::with_actor_bypass_context;
    use crate::test_helpers::setup_test_pool;
    use diesel::sql_types::Uuid as SqlUuid;
    use serde_json::json;

    const SOURCE_WS: i32 = 987_654;

    /// Run `f` on a BYPASSRLS test connection whose writes roll back.
    fn run<T>(f: impl FnOnce(&mut DbConnection) -> T) -> T {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::system("test:workspace_archive_plan");
        with_actor_bypass_context(&mut conn, &actor, |c| {
            sql_query("SET LOCAL nosdesk.in_audit_read = 'true'").execute(c)?;
            Ok::<_, diesel::result::Error>(f(c))
        })
        .expect("bypass context")
    }

    fn tag() -> String {
        Uuid::now_v7().simple().to_string()
    }

    fn workspace(c: &mut DbConnection) -> i32 {
        #[derive(QueryableByName)]
        struct Id {
            #[diesel(sql_type = Integer)]
            id: i32,
        }
        sql_query("INSERT INTO workspaces (slug, name) VALUES ($1, 'Plan target') RETURNING id")
            .bind::<Text, _>(format!("plan-{}", tag()))
            .get_result::<Id>(c)
            .expect("workspace")
            .id
    }

    fn account(c: &mut DbConnection, email: Option<&str>, verified: bool) -> Uuid {
        let uuid = Uuid::now_v7();
        sql_query("INSERT INTO users (uuid, name) VALUES ($1, 'Existing')")
            .bind::<SqlUuid, _>(uuid)
            .execute(c)
            .expect("user");
        if let Some(email) = email {
            sql_query(
                "INSERT INTO user_emails (user_uuid, email, is_primary, is_verified) \
                 VALUES ($1, $2, true, $3)",
            )
            .bind::<SqlUuid, _>(uuid)
            .bind::<Text, _>(email)
            .bind::<Bool, _>(verified)
            .execute(c)
            .expect("email");
        }
        uuid
    }

    fn member(c: &mut DbConnection, workspace_id: i32, user: Uuid, role: &str) {
        sql_query(
            "INSERT INTO workspace_members (workspace_id, user_uuid, role) VALUES ($1, $2, $3)",
        )
        .bind::<Integer, _>(workspace_id)
        .bind::<SqlUuid, _>(user)
        .bind::<Text, _>(role)
        .execute(c)
        .expect("membership");
    }

    /// An archive person.
    struct P {
        uuid: Uuid,
        email: Option<String>,
        role: Option<&'static str>,
        username: Option<String>,
        removed: bool,
    }

    fn p(email: Option<&str>, role: Option<&'static str>) -> P {
        P {
            uuid: Uuid::now_v7(),
            email: email.map(str::to_string),
            role,
            username: None,
            removed: false,
        }
    }

    fn manifest() -> WorkspaceExportManifest {
        WorkspaceExportManifest {
            workspace_export_format_version: 2,
            nosdesk_version: env!("CARGO_PKG_VERSION").to_string(),
            schema_hash: String::new(),
            migration_version: None,
            schema_sha256: None,
            created_at: String::new(),
            workspace_id: SOURCE_WS,
            workspace_slug: "source".to_string(),
            workspace_uuid: Uuid::now_v7().to_string(),
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

    fn archive(people: &[P], extra: Vec<(&str, Vec<Value>)>) -> ArchiveContents {
        let mut tables: HashMap<String, Vec<Value>> = HashMap::new();
        tables.insert(
            "workspaces".into(),
            vec![json!({"id": SOURCE_WS, "slug": "source", "name": "Source",
                        "settings": {"brand": "source"}})],
        );
        tables.insert(
            "users".into(),
            people
                .iter()
                .map(|p| {
                    json!({"uuid": p.uuid, "name": format!("Person {}", p.uuid.simple()),
                           "pronouns": null, "username": p.username,
                           "created_at": "2026-01-01T00:00:00+00:00",
                           "deleted_at": null, "avatar_url": null})
                })
                .collect(),
        );
        tables.insert(
            "user_emails".into(),
            people
                .iter()
                .filter_map(|p| {
                    let email = p.email.as_ref()?;
                    Some(
                        json!({"user_uuid": p.uuid, "email": email, "email_type": "work",
                                "is_primary": true, "is_verified": true}),
                    )
                })
                .collect(),
        );
        tables.insert(
            "workspace_members".into(),
            people
                .iter()
                .filter_map(|p| {
                    Some(json!({"workspace_id": SOURCE_WS, "user_uuid": p.uuid,
                    "role": p.role?,
                    "invited_at": "2026-01-01T00:00:00+00:00",
                    "accepted_at": "2026-01-01T00:00:00+00:00",
                    "removed_at": if p.removed {
                        json!("2026-02-01T00:00:00+00:00")
                    } else {
                        Value::Null
                    }}))
                })
                .collect(),
        );
        for (table, rows) in extra {
            tables.insert(table.to_string(), rows);
        }
        ArchiveContents {
            manifest: manifest(),
            tables,
            files: Vec::new(),
        }
    }

    fn plan_for(
        c: &mut DbConnection,
        contents: &ArchiveContents,
        workspace_id: i32,
        mode: DeploymentMode,
        runner: Runner,
    ) -> ImportPlan {
        plan_import(
            c,
            contents,
            &ImportTarget {
                workspace_id,
                mode,
                runner,
            },
            PlanOptions::default(),
        )
        .expect("plan")
    }

    fn table<'a>(plan: &'a ImportPlan, name: &str) -> &'a TableReport {
        plan.report
            .tables
            .iter()
            .find(|t| t.table == name)
            .expect("table in the report")
    }

    fn planned_rows<'a>(plan: &'a ImportPlan, name: &str) -> &'a [Row] {
        plan.tables()
            .iter()
            .find(|t| t.name == name)
            .map_or(&[], |t| t.rows.as_slice())
    }

    /// Section 3.2, self-hosted column.
    #[test]
    fn self_hosted_people_map_by_email() {
        run(|c| {
            let t = tag();
            let ws = workspace(c);
            let runner = account(c, Some(&format!("runner-{t}@example.com")), true);
            member(c, ws, runner, "owner");
            let held = account(c, Some(&format!("held-{t}@example.com")), true);
            let held_requester = account(c, Some(&format!("req-{t}@example.com")), true);

            let me = p(Some(&format!(" Runner-{t}@Example.COM ")), Some("admin"));
            let staff_held = p(Some(&format!("held-{t}@example.com")), Some("agent"));
            let staff_new = p(Some(&format!("agent-{t}@example.com")), Some("agent"));
            let other_owner = p(Some(&format!("owner-{t}@example.com")), Some("owner"));
            let requester_new = p(Some(&format!("new-{t}@example.com")), Some("member"));
            let requester_held = p(Some(&format!("req-{t}@example.com")), Some("member"));
            let mut gone = p(None, Some("agent"));
            gone.removed = true;
            let contents = archive(
                &[
                    me,
                    staff_held,
                    staff_new,
                    other_owner,
                    requester_new,
                    requester_held,
                    gone,
                ],
                vec![],
            );
            let people: Vec<Uuid> = contents.tables["users"]
                .iter()
                .map(|u| Uuid::parse_str(u["uuid"].as_str().unwrap()).unwrap())
                .collect();
            let plan = plan_for(
                c,
                &contents,
                ws,
                DeploymentMode::SelfHosted,
                Runner::SelfHostedOwner { user: runner },
            );
            assert!(plan.importable(), "{:?}", plan.report.refusals);
            let person = |i: usize| plan.person(people[i]).expect("person");

            assert_eq!(person(0).outcome, Outcome::MergedIntoRunner);
            assert_eq!(person(0).target_uuid, runner);
            assert_eq!(person(0).role_on_target.as_deref(), Some("owner"));

            assert_eq!(person(1).outcome, Outcome::Mapped);
            assert_eq!(person(1).target_uuid, held);
            assert_eq!(person(1).role_on_target.as_deref(), Some("agent"));

            assert_eq!(person(2).outcome, Outcome::Created);
            assert_eq!(person(2).target_uuid, people[2], "a free uuid is kept");

            assert_eq!(person(3).outcome, Outcome::Created);
            assert_eq!(
                person(3).role_on_target.as_deref(),
                Some("admin"),
                "an owner other than the runner comes in as admin"
            );

            assert_eq!(person(4).outcome, Outcome::Created);
            assert_eq!(person(5).outcome, Outcome::Mapped);
            assert_eq!(person(5).target_uuid, held_requester);

            assert_eq!(person(6).outcome, Outcome::Created, "no email: a new user");
            assert!(person(6).disabled);
            assert_eq!(person(6).email, None);

            // New accounts: four, never platform-privileged; emails for the
            // three with one.
            let users = plan.new_users();
            assert_eq!(users.len(), 4);
            assert!(users.iter().all(|u| u["platform_role"] == "user"));
            let emails = plan.new_emails();
            assert_eq!(emails.len(), 3);
            assert!(emails.iter().all(|(_, _, kind)| *kind == "work"));

            // Memberships keep the target's role for the runner and carry the
            // adjusted role for everyone else.
            let roles: HashMap<String, String> = planned_rows(&plan, "workspace_members")
                .iter()
                .map(|r| {
                    (
                        r["user_uuid"].as_str().unwrap().to_string(),
                        r["role"].as_str().unwrap().to_string(),
                    )
                })
                .collect();
            assert_eq!(roles.len(), 7);
            assert_eq!(roles[&runner.to_string()], "owner");
            assert_eq!(roles[&people[3].to_string()], "admin");
            assert!(planned_rows(&plan, "workspace_members")
                .iter()
                .all(|r| r["workspace_id"] == ws));
            assert_eq!(plan.report.counts.merged_into_runner, 1);
            assert_eq!(plan.report.counts.mapped, 2);
            assert_eq!(plan.report.counts.created, 4);
            assert_eq!(plan.report.counts.placeholders, 0);
        });
    }

    /// Section 3.2, hosted column.
    #[test]
    fn hosted_people_become_placeholders_unless_safe_to_map() {
        run(|c| {
            let t = tag();
            let ws = workspace(c);
            let elsewhere = workspace(c);
            let owner = account(c, Some(&format!("owner-{t}@example.com")), true);
            member(c, ws, owner, "owner");
            let baseline = account(c, Some(&format!("baseline-{t}@example.com")), false);
            member(c, elsewhere, baseline, "member");
            let _verified = account(c, Some(&format!("verified-{t}@example.com")), true);
            let staff = account(c, Some(&format!("staff-{t}@example.com")), false);
            member(c, elsewhere, staff, "agent");
            let linked = account(c, Some(&format!("linked-{t}@example.com")), false);
            sql_query(
                "INSERT INTO user_auth_identities (user_uuid, provider_type, external_id) \
                 VALUES ($1, 'oidc', $2)",
            )
            .bind::<SqlUuid, _>(linked)
            .bind::<Text, _>(format!("sub-{t}"))
            .execute(c)
            .expect("identity");
            let admin = account(c, Some(&format!("admin-{t}@example.com")), false);
            sql_query("UPDATE users SET platform_role = 'platform_admin' WHERE uuid = $1")
                .bind::<SqlUuid, _>(admin)
                .execute(c)
                .expect("platform admin");

            let contents = archive(
                &[
                    p(Some(&format!("second-{t}@example.com")), Some("owner")),
                    p(Some(&format!("OWNER-{t}@example.com")), Some("member")),
                    p(Some(&format!("agent-{t}@example.com")), Some("agent")),
                    p(Some(&format!("baseline-{t}@example.com")), Some("admin")),
                    p(Some(&format!("new-{t}@example.com")), Some("member")),
                    p(None, None),
                    p(Some(&format!("verified-{t}@example.com")), Some("member")),
                    p(Some(&format!("staff-{t}@example.com")), Some("member")),
                    p(Some(&format!("linked-{t}@example.com")), None),
                    p(Some(&format!("admin-{t}@example.com")), Some("member")),
                ],
                vec![],
            );
            let people: Vec<Uuid> = contents.tables["users"]
                .iter()
                .map(|u| Uuid::parse_str(u["uuid"].as_str().unwrap()).unwrap())
                .collect();
            let plan = plan_for(
                c,
                &contents,
                ws,
                DeploymentMode::Hosted,
                Runner::HostedOwner {
                    user: owner,
                    verified_emails: vec![
                        format!("Owner-{t}@Example.com"),
                        format!("second-{t}@example.com"),
                    ],
                },
            );
            assert!(plan.importable(), "{:?}", plan.report.refusals);
            let person = |i: usize| plan.person(people[i]).expect("person");

            // Both of the owner's verified addresses merge into them.
            for i in [0, 1] {
                assert_eq!(person(i).outcome, Outcome::MergedIntoRunner);
                assert_eq!(person(i).target_uuid, owner);
            }
            // Staff are placeholders, even when their email is held here.
            for i in [2, 3] {
                assert_eq!(person(i).outcome, Outcome::Placeholder);
                assert_eq!(person(i).placeholder, Some(PlaceholderReason::Staff));
                assert_eq!(person(i).role_on_target, None);
            }
            assert_eq!(person(4).outcome, Outcome::Created);
            assert_eq!(person(4).role_on_target.as_deref(), Some("member"));
            assert_eq!(person(5).outcome, Outcome::Placeholder);
            assert_eq!(person(5).placeholder, Some(PlaceholderReason::NoEmail));
            for i in [6, 7, 8, 9] {
                assert_eq!(person(i).outcome, Outcome::Placeholder, "person {i}");
                assert_eq!(
                    person(i).placeholder,
                    Some(PlaceholderReason::RequesterUnlinked)
                );
            }
            assert_eq!(plan.report.counts.requesters_unlinked, 4);

            // An unverified baseline account takes a requester's history.
            let requester = archive(
                &[p(
                    Some(&format!("baseline-{t}@example.com")),
                    Some("member"),
                )],
                vec![],
            );
            let mapped = plan_for(
                c,
                &requester,
                ws,
                DeploymentMode::Hosted,
                Runner::HostedOwner {
                    user: owner,
                    verified_emails: vec![],
                },
            );
            assert_eq!(mapped.people()[0].outcome, Outcome::Mapped);
            assert_eq!(mapped.people()[0].target_uuid, baseline);

            // No staff membership, none for placeholders, and placeholders get
            // no email.
            let members = planned_rows(&plan, "workspace_members");
            assert_eq!(members.len(), 1, "{members:?}");
            assert_eq!(members[0]["user_uuid"], people[4].to_string());
            assert_eq!(members[0]["role"], "member");
            let dropped = &table(&plan, "workspace_members").dropped;
            assert_eq!(dropped.get("placeholder"), Some(&5));
            assert_eq!(
                dropped.get("runner_seat"),
                Some(&2),
                "the owner's seat stays the control plane's, whatever the archive's role"
            );
            let emails = plan.new_emails();
            assert_eq!(emails.len(), 1);
            assert_eq!(emails[0].0, people[4]);
            assert_eq!(plan.new_users().len(), 8);
        });
    }

    /// An archive uuid never selects an existing account: a person whose
    /// uuid an account here has comes in as a new account, and a row naming
    /// an account the archive doesn't carry is refused.
    #[test]
    fn an_archive_uuid_never_attaches_an_existing_account() {
        run(|c| {
            for mode in [DeploymentMode::SelfHosted, DeploymentMode::Hosted] {
                let t = tag();
                let ws = workspace(c);
                let runner = account(c, Some(&format!("runner-{t}@example.com")), true);
                member(c, ws, runner, "owner");
                let existing = account(c, Some(&format!("existing-{t}@example.com")), false);
                let bystander = account(c, Some(&format!("bystander-{t}@example.com")), false);

                let mut posing = p(Some(&format!("someone-{t}@example.com")), Some("member"));
                posing.uuid = existing;
                let mut posing_runner = p(Some(&format!("other-{t}@example.com")), Some("member"));
                posing_runner.uuid = runner;
                let profile = |user: Uuid| json!({"workspace_id": SOURCE_WS, "user_uuid": user});
                let contents = archive(
                    &[posing, posing_runner],
                    vec![("user_profiles", vec![profile(existing), profile(runner)])],
                );
                let runner_kind = match mode {
                    DeploymentMode::SelfHosted => Runner::SelfHostedOwner { user: runner },
                    DeploymentMode::Hosted => Runner::HostedOwner {
                        user: runner,
                        verified_emails: vec![format!("runner-{t}@example.com")],
                    },
                };
                let plan = plan_for(c, &contents, ws, mode, runner_kind.clone());
                assert!(plan.importable(), "{:?}", plan.report.refusals);
                for archived in [existing, runner] {
                    let person = plan.person(archived).expect("person");
                    assert_eq!(person.outcome, Outcome::Created, "{mode:?}");
                    assert!(person.uuid_reminted);
                    assert_ne!(person.target_uuid, existing);
                    assert_ne!(person.target_uuid, runner);
                }
                let profiles: Vec<&str> = planned_rows(&plan, "user_profiles")
                    .iter()
                    .map(|r| r["user_uuid"].as_str().unwrap())
                    .collect();
                assert_eq!(profiles.len(), 2);
                for uuid in &profiles {
                    assert!(*uuid != existing.to_string() && *uuid != runner.to_string());
                }

                // A row naming an account the archive doesn't list.
                let contents = archive(
                    &[p(Some(&format!("lone-{t}@example.com")), Some("member"))],
                    vec![("user_profiles", vec![profile(bystander)])],
                );
                let plan = plan_for(c, &contents, ws, mode, runner_kind);
                assert!(!plan.importable());
                assert_eq!(
                    plan.report.refusals,
                    vec![PlanRefusal::UnknownPerson {
                        table: "user_profiles".into(),
                        column: "user_uuid".into(),
                        rows: 1,
                    }]
                );
            }
        });
    }

    /// F5: notification preferences are unique per account, so an existing
    /// account keeps its own; a new account brings the archive's.
    #[test]
    fn notification_preferences_load_only_for_new_accounts() {
        run(|c| {
            #[derive(QueryableByName)]
            struct Id {
                #[diesel(sql_type = Integer)]
                id: i32,
            }
            let t = tag();
            let ws = workspace(c);
            let existing = account(c, Some(&format!("existing-{t}@example.com")), false);
            let kind = sql_query("SELECT id FROM notification_types ORDER BY id LIMIT 1")
                .get_result::<Id>(c)
                .expect("a notification type")
                .id;
            sql_query(
                "INSERT INTO notification_preferences \
                   (user_uuid, notification_type_id, channel, enabled, workspace_id) \
                 VALUES ($1, $2, 'email', true, $3)",
            )
            .bind::<SqlUuid, _>(existing)
            .bind::<Integer, _>(kind)
            .bind::<Integer, _>(ws)
            .execute(c)
            .expect("existing preference");

            let mapped = p(Some(&format!("existing-{t}@example.com")), Some("member"));
            let new = p(Some(&format!("new-{t}@example.com")), Some("member"));
            let pref = |id: i32, user: Uuid| {
                json!({"id": id, "user_uuid": user, "notification_type_id": kind,
                       "channel": "email", "enabled": false, "workspace_id": SOURCE_WS})
            };
            let contents = archive(
                &[],
                vec![(
                    "notification_preferences",
                    vec![pref(1, mapped.uuid), pref(2, new.uuid)],
                )],
            );
            let contents = ArchiveContents {
                tables: {
                    let mut tables = archive(&[mapped, new], vec![]).tables;
                    tables.extend(
                        contents
                            .tables
                            .into_iter()
                            .filter(|(k, _)| k == "notification_preferences"),
                    );
                    tables
                },
                ..contents
            };
            for mode in [DeploymentMode::SelfHosted, DeploymentMode::Hosted] {
                let plan = plan_for(c, &contents, ws, mode, Runner::Operator { user: None });
                assert!(plan.importable(), "{:?}", plan.report.refusals);
                let report = table(&plan, "notification_preferences");
                assert_eq!(report.planned, 1, "{mode:?}");
                assert_eq!(report.dropped.get("existing_account"), Some(&1));
            }
        });
    }

    /// F6: usernames are unique on the instance; a taken one is dropped and
    /// reported.
    #[test]
    fn a_taken_username_is_dropped_and_reported() {
        run(|c| {
            let t = tag();
            let ws = workspace(c);
            let holder = account(c, None, false);
            let taken = format!("u{}", &t[..20]);
            sql_query("UPDATE users SET username = $1 WHERE uuid = $2")
                .bind::<Text, _>(&taken)
                .bind::<SqlUuid, _>(holder)
                .execute(c)
                .expect("username");
            let mut clash = p(Some(&format!("clash-{t}@example.com")), Some("agent"));
            clash.username = Some(taken.clone());
            let mut free = p(Some(&format!("free-{t}@example.com")), Some("agent"));
            free.username = Some(format!("f{}", &t[..20]));
            let contents = archive(&[clash, free], vec![]);
            let plan = plan_for(
                c,
                &contents,
                ws,
                DeploymentMode::SelfHosted,
                Runner::Operator { user: None },
            );
            assert!(plan.importable());
            let people = plan.people();
            assert!(people[0].username_dropped);
            assert!(!people[1].username_dropped);
            let users = plan.new_users();
            assert_eq!(users[0]["username"], Value::Null);
            assert_eq!(users[1]["username"], format!("f{}", &t[..20]));
            assert_eq!(plan.report.counts.usernames_dropped, 1);
        });
    }

    /// A row whose instance-unique value this server holds refuses the
    /// import, naming the table; new row uuids clear it. A value the archive
    /// repeats refuses it too.
    #[test]
    fn a_collision_refuses_naming_the_table() {
        run(|c| {
            #[derive(QueryableByName)]
            struct Held {
                #[diesel(sql_type = SqlUuid)]
                uuid: Uuid,
            }
            let ws = workspace(c);
            let other = workspace(c);
            let held = sql_query(
                "INSERT INTO groups (uuid, name, created_at, updated_at, mail_enabled, \
                   security_enabled, sync_enabled, workspace_id) \
                 VALUES (gen_random_uuid(), 'Held', now(), now(), false, false, false, $1) \
                 RETURNING uuid",
            )
            .bind::<Integer, _>(other)
            .get_result::<Held>(c)
            .expect("group")
            .uuid;
            let group = |id: i32, uuid: Uuid| {
                json!({"id": id, "uuid": uuid, "name": "G", "created_at": "2026-01-01T00:00:00Z",
                       "updated_at": "2026-01-01T00:00:00Z", "mail_enabled": false,
                       "security_enabled": false, "sync_enabled": false,
                       "workspace_id": SOURCE_WS})
            };
            let contents = archive(
                &[],
                vec![("groups", vec![group(1, held), group(2, Uuid::now_v7())])],
            );
            let refused = plan_for(
                c,
                &contents,
                ws,
                DeploymentMode::SelfHosted,
                Runner::Operator { user: None },
            );
            assert_eq!(
                refused.report.refusals,
                vec![PlanRefusal::Collision {
                    table: "groups".into(),
                    columns: vec!["uuid".into()],
                    rows: 1,
                }]
            );
            let message = refused.report.refusals[0].to_string();
            assert!(message.contains("groups"), "{message}");
            let shared = refused.report.refusals[0].to_refused();
            assert_eq!(shared.code, "workspace_import.collision");
            assert_eq!(shared.args["table"], "groups");
            assert!(
                refused.allocate(c).is_err(),
                "a refused plan allocates nothing"
            );

            let regenerated = plan_import(
                c,
                &contents,
                &ImportTarget {
                    workspace_id: ws,
                    mode: DeploymentMode::SelfHosted,
                    runner: Runner::Operator { user: None },
                },
                PlanOptions {
                    regenerate_row_uuids: true,
                },
            )
            .expect("plan");
            assert!(
                regenerated.importable(),
                "{:?}",
                regenerated.report.refusals
            );

            let repeated = Uuid::now_v7();
            let contents = archive(
                &[],
                vec![("groups", vec![group(1, repeated), group(2, repeated)])],
            );
            let refused = plan_for(
                c,
                &contents,
                ws,
                DeploymentMode::SelfHosted,
                Runner::Operator { user: None },
            );
            assert!(matches!(
                &refused.report.refusals[..],
                [PlanRefusal::Collision { table, .. }] if table == "groups"
            ));
        });
    }

    /// A real export planned back into the server that holds it collides on
    /// its row uuids, and each refusal names an archive table.
    #[test]
    fn an_export_planned_into_its_own_server_collides() {
        run(|c| {
            #[derive(QueryableByName)]
            struct Id {
                #[diesel(sql_type = Integer)]
                id: i32,
            }
            let src = sql_query(
                "SELECT w.id FROM workspaces w \
                 WHERE EXISTS (SELECT 1 FROM documentation_collections d WHERE d.workspace_id = w.id) \
                 ORDER BY w.id LIMIT 1",
            )
            .get_result::<Id>(c)
            .expect("a seeded workspace")
            .id;
            let archive =
                crate::services::workspace_export::export_workspace(c, src, None).expect("export");
            let contents =
                crate::services::workspace_import::read_archive(&archive, None).expect("read");
            let ws = workspace(c);
            let plan = plan_for(
                c,
                &contents,
                ws,
                DeploymentMode::SelfHosted,
                Runner::Operator { user: None },
            );
            let collisions: Vec<&str> = plan
                .report
                .refusals
                .iter()
                .filter_map(|r| match r {
                    PlanRefusal::Collision { table, .. } => Some(table.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                collisions.contains(&"documentation_collections"),
                "{collisions:?}"
            );
            assert!(collisions.iter().all(|t| contents.tables.contains_key(*t)));
        });
    }

    /// Tables the policy never carries are skipped and reported, not
    /// refused: an archive with CSV import history plans.
    #[test]
    fn import_history_in_an_archive_is_skipped() {
        run(|c| {
            let ws = workspace(c);
            let job = Uuid::now_v7();
            let contents = archive(
                &[],
                vec![
                    (
                        "import_jobs",
                        vec![json!({"id": job, "workspace_id": SOURCE_WS, "kind": "assets"})],
                    ),
                    (
                        "import_value_answers",
                        vec![
                            json!({"id": Uuid::now_v7(), "job_id": job, "workspace_id": SOURCE_WS}),
                        ],
                    ),
                ],
            );
            let plan = plan_for(
                c,
                &contents,
                ws,
                DeploymentMode::SelfHosted,
                Runner::Operator { user: None },
            );
            assert!(plan.importable(), "{:?}", plan.report.refusals);
            for name in ["import_jobs", "import_value_answers"] {
                let report = table(&plan, name);
                assert_eq!(report.class, "never");
                assert_eq!((report.in_archive, report.planned), (1, 0));
                assert!(plan.tables().iter().all(|t| t.name != name));
            }
        });
    }

    /// Keys are drawn afresh and every reference follows them; parents load
    /// before children, a table's own parents before its rows.
    #[test]
    fn allocation_rewrites_keys_and_references() {
        run(|c| {
            let ws = workspace(c);
            let person = p(
                Some(&format!("member-{}@example.com", tag())),
                Some("member"),
            );
            let who = person.uuid;
            let group = |id: i32| {
                json!({"id": id, "uuid": Uuid::now_v7(), "name": "G",
                       "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z",
                       "mail_enabled": false, "security_enabled": false, "sync_enabled": false,
                       "workspace_id": SOURCE_WS})
            };
            let contents = archive(
                &[person],
                vec![
                    ("groups", vec![group(7), group(9)]),
                    (
                        "group_includes",
                        vec![json!({"parent_group_id": 7, "child_group_id": 9,
                                    "workspace_id": SOURCE_WS})],
                    ),
                    (
                        "user_groups",
                        vec![json!({"user_uuid": who, "group_id": 9,
                                    "created_at": "2026-01-01T00:00:00Z",
                                    "workspace_id": SOURCE_WS})],
                    ),
                ],
            );
            let plan = plan_for(
                c,
                &contents,
                ws,
                DeploymentMode::SelfHosted,
                Runner::Operator { user: None },
            );
            assert!(plan.importable(), "{:?}", plan.report.refusals);
            let order: Vec<&str> = plan.tables().iter().map(|t| t.name.as_str()).collect();
            let pos = |n: &str| order.iter().position(|t| *t == n).unwrap();
            assert!(pos("groups") < pos("group_includes"));
            assert!(pos("groups") < pos("user_groups"));

            let alloc = plan.allocate(c).expect("allocate");
            let groups = plan
                .rows(
                    plan.tables().iter().find(|t| t.name == "groups").unwrap(),
                    &alloc,
                )
                .expect("rows");
            let new_id = |old: i64| {
                alloc
                    .new_key("groups", &Value::from(old))
                    .and_then(Value::as_i64)
                    .expect("allocated")
            };
            assert_ne!(new_id(7), 7);
            assert!(groups.iter().all(|g| g["workspace_id"] == ws));
            let includes = plan
                .rows(
                    plan.tables()
                        .iter()
                        .find(|t| t.name == "group_includes")
                        .unwrap(),
                    &alloc,
                )
                .expect("rows");
            assert_eq!(includes[0]["parent_group_id"], new_id(7));
            assert_eq!(includes[0]["child_group_id"], new_id(9));
            let memberships = plan
                .rows(
                    plan.tables()
                        .iter()
                        .find(|t| t.name == "user_groups")
                        .unwrap(),
                    &alloc,
                )
                .expect("rows");
            assert_eq!(memberships[0]["group_id"], new_id(9));
            assert_eq!(memberships[0]["user_uuid"], who.to_string());
        });
    }

    #[test]
    fn own_rows_load_parents_first() {
        let row = |id: i64, parent: Option<i64>| {
            json!({"id": id, "parent_id": parent})
                .as_object()
                .cloned()
                .unwrap()
        };
        let mut rows = vec![row(3, Some(2)), row(2, Some(1)), row(1, None)];
        assert!(order_own_rows(&mut rows, &["parent_id".to_string()]));
        let ids: Vec<i64> = rows.iter().map(|r| r["id"].as_i64().unwrap()).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        let mut looped = vec![row(1, Some(2)), row(2, Some(1))];
        assert!(!order_own_rows(&mut looped, &["parent_id".to_string()]));
    }

    /// #841's handover: sealed settings dropped and flagged, lookup tokens
    /// regenerated, plugin secrets left out, integrations switched off.
    #[test]
    fn handover_items_apply_and_report() {
        run(|c| {
            let ws = workspace(c);
            let token = Uuid::now_v7();
            let contents = archive(
                &[],
                vec![
                    (
                        "workspace_ldap_settings",
                        vec![json!({"workspace_id": SOURCE_WS, "enabled": true,
                                    "encrypted_bind_password": "\\x00ff",
                                    "encrypted_kek_id": 1})],
                    ),
                    (
                        "channels",
                        vec![json!({"id": 4, "workspace_id": SOURCE_WS, "enabled": true,
                                    "name": "Mail", "provider": "imap", "config": {}})],
                    ),
                    (
                        "plugins",
                        vec![
                            json!({"id": 5, "uuid": Uuid::now_v7(), "workspace_id": SOURCE_WS,
                                    "state": "installed", "name": "p", "display_name": "P",
                                    "version": "1", "manifest": {}}),
                        ],
                    ),
                    (
                        "plugin_data",
                        vec![
                            json!({"id": 1, "uuid": Uuid::now_v7(), "plugin_id": 5,
                                   "workspace_id": SOURCE_WS, "data_type": "setting",
                                   "key": "api_key", "is_secret": true, "value": "sealed"}),
                            json!({"id": 2, "uuid": Uuid::now_v7(), "plugin_id": 5,
                                   "workspace_id": SOURCE_WS, "data_type": "setting",
                                   "key": "colour", "is_secret": false, "value": "blue"}),
                        ],
                    ),
                ],
            );
            let mut contents = contents;
            contents.tables.insert(
                "tickets".into(),
                vec![
                    json!({"id": 1, "uuid": Uuid::now_v7(), "workspace_id": SOURCE_WS,
                            "guest_lookup_token": token}),
                ],
            );
            let plan = plan_for(
                c,
                &contents,
                ws,
                DeploymentMode::SelfHosted,
                Runner::Operator { user: None },
            );
            assert!(plan.importable(), "{:?}", plan.report.refusals);
            let handover = &plan.report.handover;
            assert!(handover.contains(&Handover::EncryptedSettingDropped {
                table: "workspace_ldap_settings".into(),
                column: "encrypted_bind_password".into(),
                rows: 1,
            }));
            assert!(handover.contains(&Handover::TokensRegenerated {
                table: "tickets".into(),
                column: "guest_lookup_token".into(),
                rows: 1,
            }));
            assert!(handover.contains(&Handover::PluginSecretsDropped { rows: 1 }));
            for (table, column) in [
                ("channels", "enabled"),
                ("workspace_ldap_settings", "enabled"),
                ("plugins", "state"),
            ] {
                assert!(
                    handover.contains(&Handover::SwitchedOff {
                        table: table.into(),
                        column: column.into(),
                        rows: 1,
                    }),
                    "{table}.{column}: {handover:?}"
                );
            }
            let ldap = &planned_rows(&plan, "workspace_ldap_settings")[0];
            assert_eq!(ldap["encrypted_bind_password"], Value::Null);
            assert_eq!(ldap["encrypted_kek_id"], Value::Null);
            assert_eq!(ldap["enabled"], false);
            assert_eq!(planned_rows(&plan, "plugins")[0]["state"], "disabled");
            assert_eq!(planned_rows(&plan, "plugin_data").len(), 1);
            let ticket = &planned_rows(&plan, "tickets")[0];
            assert_ne!(ticket["guest_lookup_token"], token.to_string());
        });
    }

    /// The handover lists name columns the schema has.
    #[test]
    fn every_handover_column_exists() {
        run(|c| {
            #[derive(QueryableByName)]
            struct Exists {
                #[diesel(sql_type = Bool)]
                exists: bool,
            }
            let named = SWITCHED_OFF
                .iter()
                .map(|(t, col, _, _)| (*t, *col))
                .chain(REGENERATED.iter().copied())
                .chain(USER_REFERENCES_WITHOUT_FK.iter().copied())
                .chain([PLUGIN_SECRETS]);
            for (table, column) in named {
                let found = sql_query(
                    "SELECT EXISTS (SELECT 1 FROM pg_attribute \
                     WHERE attrelid = to_regclass('public.' || $1) AND attname = $2 \
                       AND NOT attisdropped) AS exists",
                )
                .bind::<Text, _>(table)
                .bind::<Text, _>(column)
                .get_result::<Exists>(c)
                .expect("exists");
                assert!(found.exists, "{table}.{column} isn't in the schema");
                assert!(
                    policy::table(table).is_some_and(|t| t.class.carried()),
                    "{table} isn't carried"
                );
            }
        });
    }

    /// Every user reference on a carried table goes through the people map.
    /// The foreign keys are read from the catalog; a uuid column that holds
    /// a person without one fails here until it is listed in
    /// [`USER_REFERENCES_WITHOUT_FK`] (or below, when it isn't a person).
    #[test]
    fn every_user_reference_goes_through_the_people_map() {
        /// uuid columns on carried tables that don't name a person.
        const NOT_PEOPLE: &[(&str, &str)] = &[
            ("comments", "client_id"),
            ("outbound_emails", "correlation_id"),
            ("outbound_emails", "lease_token"),
            ("rule_applications", "correlation_id"),
            ("rule_applications", "originating_event_id"),
            ("site_settings", "approval_skip_by"),
            ("workspace_identity_providers", "teams_app_id"),
        ];
        run(|c| {
            #[derive(QueryableByName)]
            struct Column {
                #[diesel(sql_type = Text)]
                table_name: String,
                #[diesel(sql_type = Text)]
                column_name: String,
                #[diesel(sql_type = Bool)]
                user_fk: bool,
            }
            let columns: Vec<Column> = sql_query(
                "SELECT c.relname::text AS table_name, a.attname::text AS column_name, \
                        EXISTS (SELECT 1 FROM pg_constraint k \
                                WHERE k.contype = 'f' AND k.conrelid = c.oid \
                                  AND k.confrelid = 'public.users'::regclass \
                                  AND a.attnum = ANY (k.conkey)) AS user_fk \
                 FROM pg_class c \
                 JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public' \
                 JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
                 WHERE c.relkind IN ('r', 'p') AND NOT c.relispartition \
                   AND EXISTS (SELECT 1 FROM pg_attribute w WHERE w.attrelid = c.oid \
                               AND w.attname = 'workspace_id' AND NOT w.attisdropped) \
                   AND (format_type(a.atttypid, a.atttypmod) IN ('uuid', 'uuid[]') \
                        OR a.attname ~ '(_uuid|_by)$') \
                   AND a.attname NOT IN ('id', 'uuid')",
            )
            .load(c)
            .expect("columns");

            let remapped: HashSet<(String, String)> = user_reference_columns(c)
                .expect("user references")
                .into_iter()
                .collect();
            let fks: HashSet<(String, String)> = columns
                .iter()
                .filter(|col| col.user_fk)
                .map(|col| (col.table_name.clone(), col.column_name.clone()))
                .collect();
            assert_eq!(
                remapped, fks,
                "the plan's user references are every users.uuid foreign key on a workspace table"
            );
            let tables: HashSet<&String> = fks.iter().map(|(t, _)| t).collect();
            assert!(
                fks.len() >= 106 && tables.len() >= 79,
                "{} user foreign keys on {} tables",
                fks.len(),
                tables.len()
            );

            let listed: HashSet<(&str, &str)> = USER_REFERENCES_WITHOUT_FK
                .iter()
                .chain(NOT_PEOPLE)
                .chain(REGENERATED)
                .copied()
                .collect();
            let unclassified: Vec<String> = columns
                .iter()
                .filter(|col| !col.user_fk)
                .filter(|col| policy::carried(&col.table_name))
                .filter(|col| {
                    !listed.contains(&(col.table_name.as_str(), col.column_name.as_str()))
                })
                .map(|col| format!("{}.{}", col.table_name, col.column_name))
                .collect();
            assert!(
                unclassified.is_empty(),
                "uuid columns on carried tables that may name a person, without a foreign key: \
                 {unclassified:?}. Add each to USER_REFERENCES_WITHOUT_FK, or to NOT_PEOPLE."
            );
        });
    }

    /// A user reference without a foreign key follows the people map, and
    /// one naming someone the archive doesn't carry is cleared, never kept.
    #[test]
    fn a_user_reference_without_a_foreign_key_is_remapped_or_cleared() {
        run(|c| {
            let ws = workspace(c);
            let stranger = account(c, None, false);
            let mut known = p(Some(&format!("known-{}@example.com", tag())), Some("agent"));
            known.uuid = Uuid::now_v7();
            let who = known.uuid;
            let contents = archive(
                &[known],
                vec![(
                    "assignment_log",
                    vec![json!({"id": 1, "workspace_id": SOURCE_WS,
                               "previous_assignee_uuid": who, "new_assignee_uuid": stranger})],
                )],
            );
            let plan = plan_for(
                c,
                &contents,
                ws,
                DeploymentMode::SelfHosted,
                Runner::Operator { user: None },
            );
            assert!(plan.importable(), "{:?}", plan.report.refusals);
            let row = &planned_rows(&plan, "assignment_log")[0];
            let target = plan.person(who).unwrap().target_uuid;
            assert_eq!(row["previous_assignee_uuid"], target.to_string());
            assert_eq!(row["new_assignee_uuid"], Value::Null);
            assert_eq!(
                table(&plan, "assignment_log")
                    .cleared
                    .get("new_assignee_uuid"),
                Some(&1)
            );
        });
    }
}
