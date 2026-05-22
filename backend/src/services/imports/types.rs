//! Shared types for the import pipeline.

use serde::{Deserialize, Serialize};

use crate::db::DbConnection;

use super::csv_parser::ParsedCsv;

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

/// Dry-run output the admin reviews before committing. Errors
/// are capped at MAX_ERRORS so a million-row file with every
/// row broken doesn't ship a million-entry array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSummary {
    pub row_count: usize,
    pub would_create: usize,
    pub would_update: usize,
    pub errors: Vec<RowError>,
    /// True iff there were more errors than fit in the cap;
    /// the admin sees the count + a "showing first N" notice.
    pub errors_truncated: bool,
}

/// Per-row validation failure. Surface enough context for the
/// admin to fix the row in their source CSV and re-upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowError {
    /// 1-indexed CSV row number (header row is row 1, so data
    /// rows start at 2). Matches what spreadsheet editors show.
    pub row: usize,
    /// Column name from the header, or `None` for whole-row
    /// errors (e.g. natural-key uniqueness within the file).
    pub column: Option<String>,
    pub message: String,
}

/// Cap on the number of per-row errors a dry-run carries back.
/// Trades completeness for round-trip size + UI readability.
pub const MAX_ERRORS: usize = 100;

/// One importer per record type. Stateless: each call gets a
/// fresh borrow of the DB connection so it can be used inside
/// the commit transaction.
pub trait Importer: Send + Sync {
    /// Headers a freshly-downloaded template carries, in order.
    /// The dry-run rejects CSVs whose header set doesn't match.
    fn template_headers(&self) -> &'static [&'static str];

    /// Run validation across every row, build the summary, but
    /// don't write. Internal lookups (natural-key collisions
    /// against existing rows, FK targets, etc.) hit the DB.
    fn dry_run(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
    ) -> Result<ImportSummary, diesel::result::Error>;

    /// Apply the rows. Returns the count of rows committed.
    /// Caller wraps in a transaction.
    fn commit(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
    ) -> Result<i32, diesel::result::Error>;
}
