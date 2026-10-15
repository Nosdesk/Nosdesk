//! Bulk import from CSV or .xlsx. Three steps:
//!
//! 1. Upload parses the file (`csv_parser`, or `xlsx` for a workbook,
//!    whose sheet and header row can be chosen again), keeps its grid,
//!    and suggests which field each column feeds (`mapping`).
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
pub mod report;
pub mod safe_cells;
pub mod tickets;
pub mod types;
pub mod users;
pub mod xlsx;

pub use types::{
    ApplyTo, CatalogKind, CatalogRef, CellError, CommitError, Committed, Constant, DateOrder,
    DecimalMark, ErrorGroup, FieldChange, FieldGroup, FieldSpec, ImportContext, ImportOptions,
    ImportSummary, ImportType, ImportedRecords, ImportedUser, Importer, MatchBy, Notice, RecordId,
    RecordIds, RowError, RowOutcome, RowPhase, RowPlan, RowResult, StoredRowResults, ValueAnswer,
    ValueAnswers, ValueType,
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
    Ok(check(conn, t, parsed, ctx)?.0)
}

/// The dry run with every row's result as well as the summary.
pub fn check(
    conn: &mut DbConnection,
    t: ImportType,
    parsed: &csv_parser::ParsedCsv,
    ctx: &ImportContext,
) -> Result<(ImportSummary, Vec<RowResult>), DieselError> {
    let plan = importer_for(t).dry_run(conn, parsed, ctx)?;
    let summary = ImportSummary::from_plan(parsed.rows.len(), &plan);
    Ok((summary, plan.rows))
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
    Ok(apply(conn, t, parsed, ctx)?.records)
}

/// The commit with every row's result as well as the written records.
pub fn apply(
    conn: &mut DbConnection,
    t: ImportType,
    parsed: &csv_parser::ParsedCsv,
    ctx: &ImportContext,
) -> Result<Committed, CommitError> {
    let mut committed = importer_for(t).commit(conn, parsed, ctx)?;
    stamp_written(&mut committed.rows);
    Ok(committed)
}

/// Mark every row the commit wrote with one commit time.
fn stamp_written(rows: &mut [RowResult]) {
    let now = chrono::Utc::now();
    for row in rows {
        if matches!(row.outcome, RowOutcome::Create | RowOutcome::Update) {
            row.written_at = Some(now);
        }
    }
}

/// The dry run over a mapped grid, with the rows the mapping held back
/// (`mapping::map_grid`) merged in as skipped, in file order.
pub fn check_mapped(
    conn: &mut DbConnection,
    t: ImportType,
    mapped: &mapping::MappedGrid,
    ctx: &ImportContext,
) -> Result<(ImportSummary, Vec<RowResult>), DieselError> {
    let mut plan = importer_for(t).dry_run(conn, &mapped.grid, ctx)?;
    merge_blocked(&mut plan.rows, &mapped.blocked);
    let summary = ImportSummary::from_plan(mapped.row_count(), &plan);
    Ok((summary, plan.rows))
}

/// The commit over a mapped grid. Held-back rows were never handed to the
/// importer, so nothing is written for them; they join the row results
/// as skipped.
pub fn apply_mapped(
    conn: &mut DbConnection,
    t: ImportType,
    mapped: &mapping::MappedGrid,
    ctx: &ImportContext,
) -> Result<Committed, CommitError> {
    let mut committed = importer_for(t).commit(conn, &mapped.grid, ctx)?;
    stamp_written(&mut committed.rows);
    merge_blocked(&mut committed.rows, &mapped.blocked);
    Ok(committed)
}

fn merge_blocked(rows: &mut Vec<RowResult>, blocked: &[RowResult]) {
    if blocked.is_empty() {
        return;
    }
    rows.extend(blocked.iter().cloned());
    rows.sort_by_key(|r| r.row);
}
