//! Import the next sheet: a new draft on another sheet of a workbook an
//! import already brought in.
//!
//! The new job gets its own copy of the upload under its own id, so the
//! source job's 30-day prune and the orphan sweep, which both work by job
//! prefix, never take a file a live draft still reads. Nothing else is
//! carried: no constants, options, answers or mapping. The new job's Map
//! step suggests columns from memory like any new import (decision M2).
//!
//! Every job made from one upload names the first one in
//! `import_jobs.upload_of`, so the sheets already imported from it are the
//! sheets of its committed jobs.

use std::collections::HashSet;

use uuid::Uuid;

use super::history::{state_of, JobState};
use super::mapping::ImportSource;
use super::xlsx::FileFormat;
use crate::repository::imports::UploadJobRow;

/// Why a next sheet can't be imported from a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The job wasn't committed.
    NotImported,
    /// The prune removed the job's file.
    Pruned,
    /// A CSV, or a workbook with one sheet.
    SingleSheet,
    /// The workbook has no sheet by that name.
    UnknownSheet,
    /// The sheet is hidden in the workbook.
    HiddenSheet,
    /// An import from this upload already brought that sheet in.
    AlreadyImported,
}

/// What a request for `sheet` comes to before any file is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Make a new draft on the sheet.
    Create,
    /// A draft on the sheet from this upload is still open: go on with it.
    Existing(Uuid),
}

/// The upload a job's file came from: the first job it was sent to.
pub fn root_of(job_id: Uuid, upload_of: Option<Uuid>) -> Uuid {
    upload_of.unwrap_or(job_id)
}

/// The sheets a committed job made from this upload reads.
pub fn imported_sheets(jobs: &[UploadJobRow]) -> HashSet<String> {
    jobs.iter()
        .filter(|j| j.status == "done")
        .filter_map(|j| j.sheet.clone())
        .collect()
}

/// A draft from this upload on `sheet` that can still go on.
fn open_draft(jobs: &[UploadJobRow], sheet: &str) -> Option<Uuid> {
    let now = chrono::Utc::now();
    jobs.iter()
        .filter(|j| j.sheet.as_deref() == Some(sheet))
        .filter(|j| state_of(&j.status, j.updated_at, j.pruned_at, now) == Some(JobState::Draft))
        .map(|j| j.id)
        .next_back()
}

fn is_workbook(source: &ImportSource) -> bool {
    source.format == FileFormat::Xlsx.as_str() && source.sheets.len() > 1
}

/// Whether `sheet` can be imported next from the job (`status`, pruned or
/// not, read from `source`), and how, given every job of its upload.
pub fn plan(
    status: &str,
    pruned: bool,
    source: Option<&ImportSource>,
    sheet: &str,
    jobs: &[UploadJobRow],
) -> Result<Plan, Refusal> {
    if status != "done" {
        return Err(Refusal::NotImported);
    }
    if pruned {
        return Err(Refusal::Pruned);
    }
    let source = match source {
        Some(s) if is_workbook(s) => s,
        _ => return Err(Refusal::SingleSheet),
    };
    let info = source
        .sheets
        .iter()
        .find(|s| s.name == sheet)
        .ok_or(Refusal::UnknownSheet)?;
    if !info.visible {
        return Err(Refusal::HiddenSheet);
    }
    if imported_sheets(jobs).contains(sheet) {
        return Err(Refusal::AlreadyImported);
    }
    Ok(match open_draft(jobs, sheet) {
        Some(id) => Plan::Existing(id),
        None => Plan::Create,
    })
}

/// The sheet Done offers next: a visible sheet with rows that no committed
/// job from this upload read, the first after `source`'s own sheet in
/// workbook order, else the first before it.
pub fn offer(source: &ImportSource, jobs: &[UploadJobRow]) -> Option<String> {
    if !is_workbook(source) {
        return None;
    }
    let imported = imported_sheets(jobs);
    let current = source
        .sheet
        .as_deref()
        .and_then(|name| source.sheets.iter().position(|s| s.name == name))
        .unwrap_or(0);
    let candidates =
        source.sheets.iter().enumerate().filter(|(i, s)| {
            *i != current && s.visible && s.rows > 0 && !imported.contains(&s.name)
        });
    let (after, before): (Vec<_>, Vec<_>) = candidates.partition(|(i, _)| *i > current);
    after
        .into_iter()
        .chain(before)
        .map(|(_, s)| s.name.clone())
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::imports::mapping::SheetInfo;

    fn source(current: &str, sheets: &[(&str, bool, usize)]) -> ImportSource {
        ImportSource {
            format: "xlsx".to_string(),
            sheets: sheets
                .iter()
                .map(|(name, visible, rows)| SheetInfo {
                    name: name.to_string(),
                    visible: *visible,
                    rows: *rows,
                })
                .collect(),
            sheet: Some(current.to_string()),
            header_row: 1,
            headers: Vec::new(),
            row_count: 1,
            delimiter: None,
            preview: Vec::new(),
        }
    }

    fn job(status: &str, sheet: &str) -> UploadJobRow {
        UploadJobRow {
            id: Uuid::now_v7(),
            status: status.to_string(),
            sheet: Some(sheet.to_string()),
            updated_at: chrono::Utc::now(),
            pruned_at: None,
        }
    }

    const BOOK: &[(&str, bool, usize)] = &[
        ("Cameras", true, 412),
        ("Vehicles", true, 31),
        ("Lookups", false, 9),
        ("Notes", true, 0),
        ("Misc", true, 61),
    ];

    #[test]
    fn offers_the_next_visible_sheet_with_rows_not_yet_imported() {
        let cameras = source("Cameras", BOOK);
        let done = [job("done", "Cameras")];
        assert_eq!(offer(&cameras, &done).as_deref(), Some("Vehicles"));

        let vehicles = source("Vehicles", BOOK);
        let both = [job("done", "Cameras"), job("done", "Vehicles")];
        assert_eq!(offer(&vehicles, &both).as_deref(), Some("Misc"));

        let all = [
            job("done", "Cameras"),
            job("done", "Vehicles"),
            job("done", "Misc"),
        ];
        assert_eq!(offer(&source("Misc", BOOK), &all), None);
    }

    #[test]
    fn wraps_to_an_earlier_sheet_and_ignores_drafts() {
        let misc = source("Misc", BOOK);
        let jobs = [job("done", "Misc"), job("parsed", "Cameras")];
        assert_eq!(offer(&misc, &jobs).as_deref(), Some("Cameras"));
    }

    #[test]
    fn a_csv_or_one_sheet_offers_nothing() {
        let mut csv = source("Cameras", BOOK);
        csv.format = "csv".to_string();
        assert_eq!(offer(&csv, &[]), None);
        let one = source("Cameras", &[("Cameras", true, 4)]);
        assert_eq!(offer(&one, &[]), None);
    }

    #[test]
    fn refusals() {
        let book = source("Cameras", BOOK);
        let done = [job("done", "Cameras")];
        let ask = |status: &str, pruned: bool, sheet: &str| {
            plan(status, pruned, Some(&book), sheet, &done)
        };
        assert_eq!(
            ask("dry_run_done", false, "Vehicles"),
            Err(Refusal::NotImported)
        );
        assert_eq!(ask("done", true, "Vehicles"), Err(Refusal::Pruned));
        assert_eq!(ask("done", false, "Nope"), Err(Refusal::UnknownSheet));
        assert_eq!(ask("done", false, "Lookups"), Err(Refusal::HiddenSheet));
        assert_eq!(ask("done", false, "Cameras"), Err(Refusal::AlreadyImported));
        assert_eq!(ask("done", false, "Vehicles"), Ok(Plan::Create));
        let one = source("Cameras", &[("Cameras", true, 4)]);
        assert_eq!(
            plan("done", false, Some(&one), "Cameras", &done),
            Err(Refusal::SingleSheet)
        );
        assert_eq!(
            plan("done", false, None, "Cameras", &done),
            Err(Refusal::SingleSheet)
        );
    }

    #[test]
    fn an_open_draft_on_the_sheet_is_reused() {
        let book = source("Cameras", BOOK);
        let draft = job("dry_run_done", "Vehicles");
        let id = draft.id;
        let jobs = [job("done", "Cameras"), draft];
        assert_eq!(
            plan("done", false, Some(&book), "Vehicles", &jobs),
            Ok(Plan::Existing(id))
        );
        let mut stale = job("parsed", "Vehicles");
        stale.updated_at = chrono::Utc::now() - chrono::Duration::days(31);
        let jobs = [job("done", "Cameras"), stale];
        assert_eq!(
            plan("done", false, Some(&book), "Vehicles", &jobs),
            Ok(Plan::Create)
        );
    }
}
