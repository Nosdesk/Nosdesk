//! Shared types for the import pipeline.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models;

use super::csv_parser::ParsedCsv;

/// Entities written by an import commit, grouped by type and carrying
/// the full models. The repository writers each importer calls have
/// already emitted the per-row sync events in-transaction; the import
/// handler uses these to drive the same `spawn_index_*` search-indexing
/// the create handlers run, so imported rows are indexed identically to
/// API-created ones without a re-load.
pub enum ImportedRecords {
    Users(Vec<ImportedUser>),
    Assets(Vec<models::Asset>),
    Tickets(Vec<models::Ticket>),
}

/// A user written by the import, paired with its primary email so the
/// search indexer can populate the user doc's email metadata without a
/// second lookup.
pub struct ImportedUser {
    pub user: models::User,
    pub primary_email: Option<String>,
}

impl ImportedRecords {
    /// Number of rows committed, regardless of entity type. Feeds the
    /// import job's `records_committed`.
    pub fn len(&self) -> usize {
        match self {
            Self::Users(v) => v.len(),
            Self::Assets(v) => v.len(),
            Self::Tickets(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Closed set of importable record types. The string codes
/// match the `import_jobs.job_type` CHECK and the frontend
/// type-picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportType {
    Assets,
    Users,
    Tickets,
}

impl ImportType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Assets => "assets",
            Self::Users => "users",
            Self::Tickets => "tickets",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "assets" => Some(Self::Assets),
            "users" => Some(Self::Users),
            "tickets" => Some(Self::Tickets),
            _ => None,
        }
    }
}

/// Dry-run output the admin reviews before committing. The stored
/// `errors` list is capped at MAX_ERRORS so a million-row file with every
/// row broken doesn't ship a million-entry array; the totals and the
/// error groups count every error, and the job's row results keep every
/// row.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportSummary {
    pub row_count: usize,
    pub would_create: usize,
    pub would_update: usize,
    /// Rows that match a record and would write nothing to it.
    #[serde(default)]
    pub unchanged: usize,
    pub errors: Vec<RowError>,
    /// True iff there were more errors than fit in the cap;
    /// the admin sees the count + a "showing first N" notice.
    pub errors_truncated: bool,
    /// Every error, before the cap.
    #[serde(default)]
    pub errors_total: usize,
    /// Rows with at least one error: the rows the commit skips.
    #[serde(default)]
    pub rows_with_errors: usize,
    /// The most frequent problems: identical messages on the same column,
    /// most rows first.
    #[serde(default)]
    pub error_groups: Vec<ErrorGroup>,
    /// Things worth knowing that aren't errors.
    #[serde(default)]
    pub notices: Vec<Notice>,
    /// Values in the file that rows import without: a value for an
    /// attribute the row's kind doesn't have, a group left off. Counted per
    /// field, kind and value, most first; the rows' results name each one.
    #[serde(default)]
    pub dropped: Vec<DroppedGroup>,
    /// Catalog entries the import would create, in file order.
    #[serde(default, skip_serializing_if = "CatalogPlan::is_empty")]
    pub catalog: CatalogPlan,
    /// Fields updated rows change, each with how many rows change it, most
    /// first.
    #[serde(default)]
    pub changes: Vec<ChangeCount>,
    /// Kinds of the records the import creates, each with how many, most
    /// first. Assets only.
    #[serde(default)]
    pub kinds: Vec<KindCount>,
    /// The file's columns no field takes (none chosen, or Don't import),
    /// in source order, each with how many rows hold a value in it.
    #[serde(default)]
    pub unmapped: Vec<UnmappedColumn>,
}

/// One field updated rows change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeCount {
    /// Field id.
    pub field: String,
    /// The file's header the field was mapped from; `None` for a field a
    /// constant sets, or one the row's other cells change (a status).
    #[serde(default)]
    pub source_column: Option<String>,
    /// Rows that change it.
    pub count: usize,
    /// A constant set for every row, existing records included, writes it.
    #[serde(default)]
    pub constant: bool,
}

/// How many records of one kind an import creates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindCount {
    /// Kind slug.
    pub kind: String,
    pub count: usize,
}

/// A column of the file no field takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnmappedColumn {
    /// The source column (0-based).
    pub index: usize,
    /// Rows with a value in it.
    pub values: usize,
}

/// Catalog entries an import creates, each named as the file first spells
/// it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPlan {
    #[serde(default)]
    pub manufacturers: Vec<String>,
    #[serde(default)]
    pub models: Vec<PlannedModel>,
    #[serde(default)]
    pub groups: Vec<String>,
}

impl CatalogPlan {
    pub fn is_empty(&self) -> bool {
        self.manufacturers.is_empty() && self.models.is_empty() && self.groups.is_empty()
    }
}

/// A model an import creates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedModel {
    /// Its manufacturer's name, existing or created by the same import.
    pub manufacturer: String,
    pub name: String,
    /// Kind slug.
    pub kind: String,
}

impl ImportSummary {
    pub fn new(row_count: usize) -> Self {
        Self {
            row_count,
            ..Default::default()
        }
    }

    /// Record one error. Importers push every error; [`Self::finish`]
    /// counts and groups them, then caps the list.
    pub fn push_error(&mut self, row: usize, column: Option<String>, message: String) {
        self.errors.push(RowError {
            row,
            column,
            source_column: None,
            message,
        });
    }

    /// The summary of a checked plan: the counts and errors of its rows,
    /// a file-level error if the file couldn't be checked at all, and its
    /// notices.
    pub fn from_plan(row_count: usize, plan: &RowPlan) -> Self {
        let mut summary = Self::new(row_count);
        if let Some(message) = &plan.file_error {
            summary.push_error(1, None, message.clone());
        }
        let mut dropped: Vec<DroppedGroup> = Vec::new();
        for result in &plan.rows {
            if result.outcome != RowOutcome::Skipped {
                for cell in &result.dropped {
                    match dropped.iter_mut().find(|g| {
                        g.column == cell.column && g.kind == cell.kind && g.value == cell.value
                    }) {
                        Some(group) => group.count += 1,
                        None => dropped.push(DroppedGroup {
                            column: cell.column.clone(),
                            source_column: cell.source_column.clone(),
                            kind: cell.kind.clone(),
                            value: cell.value.clone(),
                            count: 1,
                        }),
                    }
                }
            }
            match result.outcome {
                RowOutcome::Create => summary.would_create += 1,
                RowOutcome::Update => summary.would_update += 1,
                RowOutcome::Unchanged => summary.unchanged += 1,
                RowOutcome::Skipped => {}
            }
            for e in &result.errors {
                summary.errors.push(RowError {
                    row: result.row,
                    column: e.column.clone(),
                    source_column: e.source_column.clone(),
                    message: e.message.clone(),
                });
            }
        }
        summary.notices = plan.notices.clone();
        summary.catalog = plan.catalog.clone();
        summary.changes = change_counts(&plan.rows);
        summary.kinds = kind_counts(&plan.rows);
        // Stable: equal counts keep file order.
        dropped.sort_by_key(|g| std::cmp::Reverse(g.count));
        summary.dropped = dropped;
        summary.finish()
    }

    /// Count the errors, group them, and cap the stored list.
    pub fn finish(mut self) -> Self {
        self.errors_total = self.errors.len();
        let rows: std::collections::HashSet<usize> = self.errors.iter().map(|e| e.row).collect();
        self.rows_with_errors = rows.len();

        let mut groups: Vec<ErrorGroup> = Vec::new();
        let mut index: std::collections::HashMap<(Option<&str>, &str), usize> =
            std::collections::HashMap::new();
        for e in &self.errors {
            let key = (e.column.as_deref(), e.message.as_str());
            match index.get(&key) {
                Some(&i) => groups[i].count += 1,
                None => {
                    index.insert(key, groups.len());
                    groups.push(ErrorGroup {
                        column: e.column.clone(),
                        source_column: e.source_column.clone(),
                        message: e.message.clone(),
                        count: 1,
                        fix: None,
                    });
                }
            }
        }
        // Stable: equal counts keep file order.
        groups.sort_by_key(|g| std::cmp::Reverse(g.count));
        groups.truncate(MAX_ERROR_GROUPS);
        self.error_groups = groups;

        if self.errors.len() > MAX_ERRORS {
            self.errors.truncate(MAX_ERRORS);
            self.errors_truncated = true;
        }
        self
    }
}

/// Per field, how many updated rows change it, most first; equal counts
/// keep the order fields first appear in.
fn change_counts(rows: &[RowResult]) -> Vec<ChangeCount> {
    let mut out: Vec<ChangeCount> = Vec::new();
    for row in rows.iter().filter(|r| r.outcome == RowOutcome::Update) {
        for field in row.fields_written.keys() {
            match out.iter_mut().find(|c| &c.field == field) {
                Some(change) => change.count += 1,
                None => out.push(ChangeCount {
                    field: field.clone(),
                    source_column: None,
                    count: 1,
                    constant: false,
                }),
            }
        }
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.count));
    out
}

/// Per kind, how many created rows take it, most first.
fn kind_counts(rows: &[RowResult]) -> Vec<KindCount> {
    let mut out: Vec<KindCount> = Vec::new();
    for row in rows.iter().filter(|r| r.outcome == RowOutcome::Create) {
        let Some(kind) = &row.kind else {
            continue;
        };
        match out.iter_mut().find(|k| &k.kind == kind) {
            Some(count) => count.count += 1,
            None => out.push(KindCount {
                kind: kind.clone(),
                count: 1,
            }),
        }
    }
    out.sort_by_key(|k| std::cmp::Reverse(k.count));
    out
}

/// Something about the plan that isn't an error, counted. The client
/// words it from `code`; `message` is the English fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub code: String,
    pub count: usize,
    pub message: String,
}

/// Values of one field a kind doesn't have, dropped from that many rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroppedGroup {
    /// Field id.
    pub column: String,
    /// The file's header the field was mapped from.
    #[serde(default)]
    pub source_column: Option<String>,
    /// The rows' kind (slug).
    pub kind: String,
    /// The value left out, for a field whose values are left out one by one
    /// (a group nothing links); `None` for an attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub count: usize,
}

/// A value one row imports without: its kind doesn't have the field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroppedCell {
    /// Field id.
    pub column: String,
    #[serde(default)]
    pub source_column: Option<String>,
    /// The row's kind (slug).
    pub kind: String,
    /// The value left out, as [`DroppedGroup::value`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// What one row of the file comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowOutcome {
    Create,
    Update,
    /// Matches a record and writes nothing to it.
    Unchanged,
    /// Has an error, so nothing is written for it.
    Skipped,
}

/// A problem with one row, on one field or the whole row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellError {
    /// Field id, or `None` for a whole-row error.
    pub column: Option<String>,
    /// The file's header the field was mapped from.
    #[serde(default)]
    pub source_column: Option<String>,
    pub message: String,
}

/// The record a row matched or wrote: an integer id (assets, tickets) or
/// a uuid (users).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RecordId {
    Int(i32),
    Uuid(Uuid),
}

/// One row's result, kept for every row in `import_jobs.row_results`.
///
/// Every member is named and optional beyond `row` and `outcome`, so the
/// record grows (what a write changed, when it was written) without
/// rewriting stored jobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowResult {
    /// The row's line in the file (`SourceRow::source_row`).
    pub row: usize,
    pub outcome: RowOutcome,
    /// The value the row is matched on (asset tag or serial number,
    /// email), when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// What the row is called (asset name, person, ticket title).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The record's kind (slug), for an asset row that checks out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Before the commit, the record an update or unchanged row matches;
    /// after it, the record the row created or updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<RecordId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<CellError>,
    /// Values the row imports without, because its kind doesn't have the
    /// field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<DroppedCell>,
    /// The row's cells as the file has them, in source column order, kept
    /// for skipped rows so the error report can give them back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<String>,
    /// After the commit, each field the row wrote with its value before and
    /// after, as text. Only fields whose value changed; a blank cell never
    /// writes, so it is never here. In a checked plan, each field an update
    /// would change (a new record's: the status move the commit would make),
    /// which the commit holds up against the plan it makes again.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields_written: BTreeMap<String, FieldChange>,
    /// Groups the row added the record to (ids).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups_added: Vec<i32>,
    /// Catalog entries the row created.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub catalog_created: Vec<CatalogRef>,
    /// The last lifecycle event the commit wrote for the record: the status
    /// move a row made through the lifecycle rules, else the Imported event
    /// of a created asset. Every event an import writes names the job in
    /// its metadata (`import_job_id`), so the record is untouched since
    /// while this is its newest event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_id: Option<i32>,
    /// When the commit wrote the row; one time for the whole job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The record's `updated_at` as read back after the commit's last write
    /// to it, in the record's own form. A later edit moves it, which is how
    /// an import tells whether its record has been touched since.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_updated_at: Option<String>,
}

/// One field's value before and after a write, as text; `None` is blank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldChange {
    pub before: Option<String>,
    pub after: Option<String>,
}

/// A catalog entry an import created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogRef {
    pub kind: CatalogKind,
    pub id: i32,
    /// Its `updated_at` as read back in the commit, like
    /// [`RowResult::observed_updated_at`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_updated_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogKind {
    Manufacturer,
    Model,
    Group,
}

impl RowResult {
    pub fn planned(row: usize, outcome: RowOutcome) -> Self {
        Self {
            row,
            outcome,
            key: None,
            label: None,
            kind: None,
            record_id: None,
            errors: Vec::new(),
            dropped: Vec::new(),
            cells: Vec::new(),
            fields_written: BTreeMap::new(),
            groups_added: Vec::new(),
            catalog_created: Vec::new(),
            transition_id: None,
            written_at: None,
            observed_updated_at: None,
        }
    }

    /// A row skipped for `errors`, each `(field id, message)`.
    pub fn skipped(row: usize, errors: Vec<(Option<String>, String)>) -> Self {
        let mut result = Self::planned(row, RowOutcome::Skipped);
        result.errors = errors
            .into_iter()
            .map(|(column, message)| CellError {
                column,
                source_column: None,
                message,
            })
            .collect();
        result
    }

    pub fn with_key(mut self, key: Option<String>) -> Self {
        self.key = key.filter(|k| !k.is_empty());
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.label = (!label.is_empty()).then_some(label);
        self
    }

    pub fn with_record(mut self, id: RecordId) -> Self {
        self.record_id = Some(id);
        self
    }
}

/// The records a commit created and updated, as `import_jobs.record_ids`
/// keeps them once the row results are gone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordIds {
    pub created: Vec<RecordId>,
    pub updated: Vec<RecordId>,
}

impl RecordIds {
    pub fn from_rows(rows: &[RowResult]) -> Self {
        let ids = |outcome: RowOutcome| {
            rows.iter()
                .filter(|r| r.outcome == outcome)
                .filter_map(|r| r.record_id)
                .collect()
        };
        Self {
            created: ids(RowOutcome::Create),
            updated: ids(RowOutcome::Update),
        }
    }
}

/// Which run wrote the stored row results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowPhase {
    /// The dry run: what the commit would do.
    Checked,
    /// The commit: what it did.
    Committed,
}

/// What `import_jobs.row_results` holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredRowResults {
    pub phase: RowPhase,
    pub rows: Vec<RowResult>,
}

/// An importer's verdict on every row, before or after writing.
#[derive(Debug, Clone, Default)]
pub struct RowPlan {
    /// Set when the file can't be checked at all; no row has a result.
    pub file_error: Option<String>,
    /// One per data row, in file order.
    pub rows: Vec<RowResult>,
    pub notices: Vec<Notice>,
    /// Catalog entries the rows create.
    pub catalog: CatalogPlan,
}

/// What a commit wrote: the records for search indexing and every row's
/// result.
pub struct Committed {
    pub records: ImportedRecords,
    pub rows: Vec<RowResult>,
    /// The entries the commit created for values answered Create, so the
    /// answer is saved as a link to what was made.
    pub created: Vec<CreatedLink>,
}

/// A catalog entry a commit created for one answered value: the field,
/// context and value (as answers key them) whose Create made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedLink {
    pub field: String,
    pub context: String,
    pub key: String,
    pub id: i32,
}

/// One problem shared by several rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorGroup {
    /// Field id, or `None` for whole-row errors.
    pub column: Option<String>,
    /// The file's header for that field, when it came through a mapping.
    #[serde(default)]
    pub source_column: Option<String>,
    pub message: String,
    pub count: usize,
    /// Where the admin fixes it, when the summary was read against the
    /// job's mapping and answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

/// Where a problem shared by several rows is fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fix {
    /// An answer to the value on Match values.
    MatchValues,
    /// A column or a constant on Map columns.
    Map,
    /// In the file: the rows are downloaded, fixed and uploaded again.
    Download,
}

/// Per-row validation failure. Surface enough context for the
/// admin to fix the row in their source CSV and re-upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowError {
    /// The row's line in the file (`SourceRow::source_row`; the header
    /// is line 1).
    pub row: usize,
    /// Field id, or `None` for whole-row errors (e.g. natural-key
    /// uniqueness within the file).
    pub column: Option<String>,
    /// The file's header the field was mapped from, so the admin finds
    /// the cell under the name their spreadsheet uses.
    #[serde(default)]
    pub source_column: Option<String>,
    pub message: String,
}

/// Why a commit stopped. `Row` is a write the database refused for one
/// row, which the admin can fix in the file; `Db` is anything else.
#[derive(Debug)]
pub enum CommitError {
    Row {
        row: usize,
        /// Field id the refusal is about, when it is about one.
        column: Option<String>,
        message: String,
    },
    Db(diesel::result::Error),
}

impl From<diesel::result::Error> for CommitError {
    fn from(e: diesel::result::Error) -> Self {
        Self::Db(e)
    }
}

impl std::fmt::Display for CommitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Row { row, message, .. } => write!(f, "row {row}: {message}"),
            Self::Db(e) => write!(f, "{e}"),
        }
    }
}

/// The error for a key (`what` names it: "tag", "email") that several
/// rows of the file hold: each of them is skipped, and the message names
/// the rows so the admin finds them together.
pub fn duplicate_in_file(what: &str, value: &str, rows: &[usize]) -> String {
    const LISTED: usize = 5;
    let times = match rows.len() {
        2 => "twice".to_string(),
        n => format!("{n} times"),
    };
    let mut listed: Vec<String> = rows.iter().take(LISTED).map(ToString::to_string).collect();
    let rest = rows.len().saturating_sub(LISTED);
    let rows_text = if rest > 0 {
        format!("{} and {rest} more", listed.join(", "))
    } else {
        let last = listed.pop().unwrap_or_default();
        if listed.is_empty() {
            last
        } else {
            format!("{} and {last}", listed.join(", "))
        }
    };
    format!("{what} '{value}' appears {times} in this file (rows {rows_text})")
}

/// Cap on the number of per-row errors a dry-run carries back.
/// Trades completeness for round-trip size + UI readability.
pub const MAX_ERRORS: usize = 100;

/// Most error groups a summary carries.
pub const MAX_ERROR_GROUPS: usize = 10;

/// Who is importing, which job, and the options the admin chose at the
/// Map step. Built by the mapping and commit handlers.
#[derive(Debug, Clone)]
pub struct ImportContext {
    /// The admin applying the import; `None` only for callers with no
    /// signed-in user (tests).
    pub actor: Option<Uuid>,
    pub job_id: Uuid,
    pub options: ImportOptions,
}

/// How rows find the record they update. Assets only; users match by
/// email and tickets always create.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchBy {
    #[default]
    AssetTag,
    SerialNumber,
    /// Read only, from jobs and clients before [`ImportMode`]: create new
    /// assets only, matched by tag. [`ImportOptions::normalize`] rewrites
    /// it to `asset_tag` with [`ImportMode::CreateOnly`].
    None,
}

/// Which rows of the file write: rows that match an existing record
/// update it, the others create one. Assets only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    /// Matched rows update, the others create.
    #[default]
    CreateUpdate,
    /// Unmatched rows create; a matched row is skipped as already there.
    CreateOnly,
    /// Matched rows update; an unmatched row is skipped as not found.
    UpdateOnly,
}

/// How dates typed as text are read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateOrder {
    #[default]
    Iso,
    Dmy,
    Mdy,
}

/// The decimal mark of number cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecimalMark {
    #[default]
    #[serde(rename = ".")]
    Dot,
    #[serde(rename = ",")]
    Comma,
}

/// Choices the admin makes at the Map step, stored on the job and used by
/// the dry run and the commit alike.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImportOptions {
    pub match_by: MatchBy,
    pub mode: ImportMode,
    /// Kind slug for new assets whose kind cell is blank.
    pub default_kind: Option<String>,
    pub date_order: DateOrder,
    /// `None` until the Map step resolves it from the file's delimiter.
    pub decimal: Option<DecimalMark>,
    /// Values set for every row on fields no column feeds.
    pub constants: Vec<Constant>,
    /// The answers of lookup columns (manufacturer, model, groups) the rows
    /// take, set by `values::settle` and never stored: their links name
    /// entries by id, which a cell can't.
    #[serde(skip)]
    pub lookups: ValueAnswers,
}

impl ImportOptions {
    /// Options as a job stores them. Jobs stored before catalog values were
    /// answered one by one carry `create_missing`, which nothing reads now;
    /// it is dropped here so they still load, while requests that send it
    /// are refused.
    pub fn from_stored(mut value: serde_json::Value) -> serde_json::Result<Self> {
        if let Some(fields) = value.as_object_mut() {
            fields.remove("create_missing");
        }
        serde_json::from_value(value)
    }

    pub fn decimal_mark(&self) -> DecimalMark {
        self.decimal.unwrap_or_default()
    }

    /// Rewrite the legacy `match_by: none` as what it meant: match by tag
    /// and create new assets only. Refused alongside a mode that updates.
    pub fn normalize(&mut self) -> Result<(), String> {
        if self.match_by != MatchBy::None {
            return Ok(());
        }
        if self.mode == ImportMode::UpdateOnly {
            return Err(
                "match_by 'none' creates new assets only and can't be combined with mode 'update_only'; use match_by 'asset_tag' or 'serial_number'"
                    .to_string(),
            );
        }
        self.match_by = MatchBy::AssetTag;
        self.mode = ImportMode::CreateOnly;
        Ok(())
    }

    /// The key rows are matched on: `match_by`, with the legacy `none` read
    /// as the tag.
    pub fn key(&self) -> MatchBy {
        match self.match_by {
            MatchBy::None => MatchBy::AssetTag,
            key => key,
        }
    }

    /// Which rows write, with the legacy `match_by: none` read as create
    /// only.
    pub fn row_mode(&self) -> ImportMode {
        match self.match_by {
            MatchBy::None => ImportMode::CreateOnly,
            _ => self.mode,
        }
    }

    /// Clear, on a row that matches an existing record, the fields only
    /// new records take a constant for, so the row leaves them as they are.
    pub fn drop_new_only_constants(&self, row: &mut std::collections::HashMap<String, String>) {
        for constant in &self.constants {
            if constant.apply_to == ApplyTo::New {
                if let Some(cell) = row.get_mut(&constant.field) {
                    cell.clear();
                }
            }
        }
    }
}

/// A value for one field on every row, typed like a cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constant {
    /// Field id.
    pub field: String,
    pub value: String,
    #[serde(default)]
    pub apply_to: ApplyTo,
}

/// Which rows a constant writes to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyTo {
    /// Rows that create a record.
    #[default]
    New,
    /// Rows that create a record and rows that update one.
    All,
}

/// The admin's answer for each distinct source value, by field id, then
/// context (the manufacturer answer for models, the kind for enum
/// attributes, else empty), then the source value.
pub type ValueAnswers = BTreeMap<String, BTreeMap<String, BTreeMap<String, ValueAnswer>>>;

/// What happens to the rows holding one source value of a field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ValueAnswer {
    /// Link to an existing entry: an id, a slug or an option key.
    Match { target: serde_json::Value },
    /// Create the entry, named by the source value.
    Create,
    /// Keep the value as text, unlinked.
    KeepText,
    /// Write nothing for the field: leave it blank on a new record and as it
    /// is on an existing one.
    LeaveBlank,
    /// Skip the rows holding the value.
    Skip,
    /// Write this text instead of the source value.
    Replace { value: String },
}

/// Which part of the target picker a field sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldGroup {
    Core,
    Links,
    Lifecycle,
    Attribute,
}

/// How a field's cells are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Text,
    Enum,
    Number,
    Date,
    DateTime,
    Boolean,
    Array,
    Email,
    User,
    AssetRef,
}

/// One field a column can map to. Static fields carry a `label_key` the
/// client translates; attribute fields (later) carry their schema title in
/// `label` only.
#[derive(Debug, Clone, Serialize)]
pub struct FieldSpec {
    pub id: String,
    /// en-US label, also what header matching compares against.
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_key: Option<String>,
    pub group: FieldGroup,
    pub value_type: ValueType,
    pub required: bool,
    /// Kind slugs that define the field (attributes only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<String>>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    /// Expected text form, e.g. `YYYY-MM-DD`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Kind slugs whose new assets need the field (attributes only). It
    /// never blocks a mapping: rows of those kinds that lack it are
    /// reported on Review.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_for: Option<Vec<String>>,
}

impl FieldSpec {
    /// A static field of `import_type`, labelled through the
    /// `import-field-<type>-<id>` catalogue key.
    pub fn fixed(
        import_type: ImportType,
        id: &str,
        label: &str,
        group: FieldGroup,
        value_type: ValueType,
        required: bool,
    ) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            label_key: Some(format!(
                "import-field-{}-{}",
                import_type.as_str(),
                id.replace('_', "-")
            )),
            group,
            value_type,
            required,
            kinds: None,
            enum_values: None,
            format: None,
            required_for: None,
        }
    }

    pub fn with_enum(mut self, values: Vec<String>) -> Self {
        self.enum_values = Some(values);
        self
    }

    pub fn with_format(mut self, format: &str) -> Self {
        self.format = Some(format.to_string());
        self
    }
}

/// One importer per record type. Stateless: each call gets a
/// fresh borrow of the DB connection so it can be used inside
/// the commit transaction.
pub trait Importer: Send + Sync {
    /// Headers a freshly-downloaded template carries, in order.
    /// The dry-run rejects CSVs whose header set doesn't match.
    fn template_headers(&self) -> &'static [&'static str];

    /// Every field a column can map to, in picker order. The mapped grid
    /// carries one column per field, named by its id.
    fn fields(&self, conn: &mut DbConnection) -> Result<Vec<FieldSpec>, diesel::result::Error>;

    /// Other header spellings for a field, normalized (see
    /// `mapping::normalize`), as `(header, field id)`.
    fn synonyms(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }

    /// What a value of `field` can link to on the Match values step, or
    /// `None` when its values aren't a closed set. By default an enum
    /// field's options, each its own label.
    fn value_choices(
        &self,
        _conn: &mut DbConnection,
        field: &FieldSpec,
    ) -> Result<Option<Vec<super::values::Choice>>, diesel::result::Error> {
        Ok(super::values::enum_choices(field))
    }

    /// For a field whose values are answered per row context, each
    /// context with the field as its rows read it; `None` for a field
    /// answered the same on every row. Asset attributes are answered per
    /// kind.
    fn value_contexts(
        &self,
        _conn: &mut DbConnection,
        _field: &FieldSpec,
    ) -> Result<Option<Vec<super::values::ValueContext>>, diesel::result::Error> {
        Ok(None)
    }

    /// The family of contexts a field is answered per (an asset
    /// attribute per `kind`, a model per `manufacturer`), or `None` for a
    /// field answered the same on every row. A field of a family that
    /// [`Self::value_contexts`] gives no contexts for is answered per the
    /// contexts rows hold, through [`Self::lookups`].
    fn context_family(&self, _field: &FieldSpec) -> Option<&'static str> {
        None
    }

    /// Context families in the order they are settled: a family's row
    /// contexts may read the answers of the families before it.
    fn context_families(&self) -> &'static [&'static str] {
        &[]
    }

    /// For a field whose values link to catalog entries, the entries in
    /// each of `contexts` (just the empty one for a field without
    /// context); `None` for any other field.
    fn lookups(
        &self,
        _conn: &mut DbConnection,
        _field: &FieldSpec,
        _contexts: &[String],
    ) -> Result<Option<Vec<super::values::LookupContext>>, diesel::result::Error> {
        Ok(None)
    }

    /// Each row's context in `family` by source row, read from a mapped
    /// grid whose answers so far are written in (and, for lookups, in
    /// `options`). A row with none is left out.
    fn row_contexts(
        &self,
        _conn: &mut DbConnection,
        _grid: &ParsedCsv,
        _options: &ImportOptions,
        _family: &str,
    ) -> Result<std::collections::HashMap<usize, String>, diesel::result::Error> {
        Ok(std::collections::HashMap::new())
    }

    /// Other names for a field's choices, as `(name, choice value)`: a
    /// value spelled this way is Guessed as the choice.
    fn value_synonyms(&self, _field: &str) -> &'static [(&'static str, &'static str)] {
        &[]
    }

    /// Validate every row and say what the commit would do with it, but
    /// don't write. Internal lookups (natural-key collisions
    /// against existing rows, FK targets, etc.) hit the DB. `ctx` is
    /// the admin who will commit, for checks that depend on who acts.
    fn dry_run(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        ctx: &ImportContext,
    ) -> Result<RowPlan, diesel::result::Error>;

    /// Apply the rows through the canonical repository writers (which
    /// emit the per-row sync events + enforce entity invariants in the
    /// transaction), returning the written models so the caller can
    /// index them into search, and every row's result. Caller wraps in a
    /// transaction.
    fn commit(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        ctx: &ImportContext,
    ) -> Result<Committed, CommitError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_options_drop_the_legacy_create_missing() {
        let stored = serde_json::json!({ "create_missing": true, "mode": "create_only" });
        let options = ImportOptions::from_stored(stored.clone()).expect("stored options load");
        assert_eq!(options.mode, ImportMode::CreateOnly);
        let written = serde_json::to_value(&options).expect("serialize");
        assert!(written.get("create_missing").is_none(), "{written}");
        // A request is read strictly.
        assert!(serde_json::from_value::<ImportOptions>(stored).is_err());
    }

    #[test]
    fn duplicate_message_names_the_rows() {
        assert_eq!(
            duplicate_in_file("tag", "T-1", &[31, 212]),
            "tag 'T-1' appears twice in this file (rows 31 and 212)"
        );
        assert_eq!(
            duplicate_in_file("email", "a@b.co", &[2, 5, 9]),
            "email 'a@b.co' appears 3 times in this file (rows 2, 5 and 9)"
        );
        assert_eq!(
            duplicate_in_file("tag", "T", &[1, 2, 3, 4, 5, 6, 7]),
            "tag 'T' appears 7 times in this file (rows 1, 2, 3, 4, 5 and 2 more)"
        );
    }

    #[test]
    fn summary_counts_come_from_the_rows() {
        let plan = RowPlan {
            file_error: None,
            rows: vec![
                RowResult::planned(2, RowOutcome::Create),
                RowResult::planned(3, RowOutcome::Update),
                RowResult::planned(4, RowOutcome::Unchanged),
                RowResult::skipped(
                    5,
                    vec![
                        (Some("name".into()), "name is required".into()),
                        (None, "whole row".into()),
                    ],
                ),
            ],
            notices: vec![],
            catalog: CatalogPlan::default(),
        };
        let summary = ImportSummary::from_plan(4, &plan);
        assert_eq!(
            (
                summary.would_create,
                summary.would_update,
                summary.unchanged,
                summary.rows_with_errors,
                summary.errors_total
            ),
            (1, 1, 1, 1, 2)
        );
        assert_eq!(summary.errors[0].row, 5);
    }

    #[test]
    fn row_results_keep_their_shape() {
        let row = RowResult::planned(7, RowOutcome::Update)
            .with_key(Some("T-1".into()))
            .with_label("Camera")
            .with_record(RecordId::Int(12));
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"row": 7, "outcome": "update", "key": "T-1", "label": "Camera", "record_id": 12})
        );
        let back: RowResult = serde_json::from_value(json).unwrap();
        assert_eq!(back, row);
        let user: RowResult = serde_json::from_value(serde_json::json!({
            "row": 2, "outcome": "create", "record_id": "6f1c2a52-7c1e-4b1b-9a43-6c1f8c1e2d10"
        }))
        .unwrap();
        assert!(matches!(user.record_id, Some(RecordId::Uuid(_))));
    }
}
