//! Shared types for the import pipeline.

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
/// error groups count every error.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportSummary {
    pub row_count: usize,
    pub would_create: usize,
    pub would_update: usize,
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
                        source_column: None,
                        message: e.message.clone(),
                        count: 1,
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
    Row { row: usize, message: String },
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
            Self::Row { row, message } => write!(f, "row {row}: {message}"),
            Self::Db(e) => write!(f, "{e}"),
        }
    }
}

/// Cap on the number of per-row errors a dry-run carries back.
/// Trades completeness for round-trip size + UI readability.
pub const MAX_ERRORS: usize = 100;

/// Most error groups a summary carries.
pub const MAX_ERROR_GROUPS: usize = 10;

/// Who is committing, which job, and the options the admin chose at the
/// Map step. Built by the commit handler.
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
    /// Every row creates; a row matching an existing asset is an error.
    None,
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
    /// Create manufacturers, models and groups that don't exist. Stored
    /// now; read once the catalog columns land.
    pub create_missing: bool,
    /// Kind slug for new assets whose kind cell is blank.
    pub default_kind: Option<String>,
    pub date_order: DateOrder,
    /// `None` until the Map step resolves it from the file's delimiter.
    pub decimal: Option<DecimalMark>,
}

impl ImportOptions {
    pub fn decimal_mark(&self) -> DecimalMark {
        self.decimal.unwrap_or_default()
    }
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

    /// Run validation across every row, build the summary, but
    /// don't write. Internal lookups (natural-key collisions
    /// against existing rows, FK targets, etc.) hit the DB.
    fn dry_run(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        options: &ImportOptions,
    ) -> Result<ImportSummary, diesel::result::Error>;

    /// Apply the rows through the canonical repository writers (which
    /// emit the per-row sync events + enforce entity invariants in the
    /// transaction), returning the written models so the caller can
    /// index them into search. Caller wraps in a transaction.
    fn commit(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        ctx: &ImportContext,
    ) -> Result<ImportedRecords, CommitError>;
}
