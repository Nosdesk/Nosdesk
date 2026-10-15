//! CSV bulk-import. Three steps:
//!
//! 1. Upload parses the file, keeps its grid, and suggests which
//!    field each column feeds (`mapping`).
//! 2. The admin's mapping runs the dry-run: every row is validated
//!    against the type's schema and the projected effect (would-
//!    create / would-update / errors) is reported without writing.
//! 3. Commit applies the validated rows in one transaction,
//!    upserting by the type's natural key.
//!
//! Per-type behaviour lives in a [`Importer`] implementation;
//! the top-level handlers dispatch on `ImportType`.

pub mod assets;
pub mod csv_parser;
pub mod mapping;
pub mod safe_cells;
pub mod tickets;
pub mod types;
pub mod users;

pub use types::{
    CommitError, DateOrder, DecimalMark, ErrorGroup, FieldGroup, FieldSpec, ImportContext,
    ImportOptions, ImportSummary, ImportType, ImportedRecords, ImportedUser, Importer, MatchBy,
    RowError, ValueType,
};

use diesel::result::Error as DieselError;

use crate::db::DbConnection;

/// Look up the importer implementation for a type code.
pub fn importer_for(t: ImportType) -> Box<dyn Importer> {
    match t {
        ImportType::Assets => Box::new(assets::AssetImporter),
        ImportType::Users => Box::new(users::UserImporter),
        ImportType::Tickets => Box::new(tickets::TicketImporter),
    }
}

/// Run the dry-run validation against an already-parsed CSV with the
/// options in `ctx`. The summary is the structured result the admin
/// reviews before committing.
pub fn dry_run(
    conn: &mut DbConnection,
    t: ImportType,
    parsed: &csv_parser::ParsedCsv,
    ctx: &ImportContext,
) -> Result<ImportSummary, DieselError> {
    Ok(importer_for(t).dry_run(conn, parsed, ctx)?.finish())
}

/// Apply the rows in a single transaction. Caller is
/// responsible for marking the import_jobs row's status
/// transition. Returns the written models for post-commit
/// search indexing.
pub fn commit(
    conn: &mut DbConnection,
    t: ImportType,
    parsed: &csv_parser::ParsedCsv,
    ctx: &ImportContext,
) -> Result<ImportedRecords, CommitError> {
    importer_for(t).commit(conn, parsed, ctx)
}
