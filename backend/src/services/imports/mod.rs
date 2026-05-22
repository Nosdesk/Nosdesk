//! CSV bulk-import. Two-phase workflow:
//!
//! 1. Upload + parse + dry-run validates every row against the
//!    type's schema and reports the projected effect (would-
//!    create / would-update / errors) without writing.
//! 2. Commit applies the validated rows in one transaction,
//!    upserting by the type's natural key.
//!
//! Per-type behaviour lives in a [`Importer`] implementation;
//! the top-level handlers dispatch on `ImportType`.

pub mod assets;
pub mod csv_parser;
pub mod tickets;
pub mod types;
pub mod users;

pub use types::{ImportSummary, ImportType, Importer, RowError};

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

/// Run the dry-run validation against an already-parsed CSV.
/// The summary is the structured result the admin reviews
/// before committing.
pub fn dry_run(
    conn: &mut DbConnection,
    t: ImportType,
    parsed: &csv_parser::ParsedCsv,
) -> Result<ImportSummary, DieselError> {
    importer_for(t).dry_run(conn, parsed)
}

/// Apply the rows in a single transaction. Caller is
/// responsible for marking the import_jobs row's status
/// transition.
pub fn commit(
    conn: &mut DbConnection,
    t: ImportType,
    parsed: &csv_parser::ParsedCsv,
) -> Result<i32, DieselError> {
    importer_for(t).commit(conn, parsed)
}
