//! .xlsx uploads: from the uploaded bytes to the grid the rest of the
//! import reads, with the sheet and header row the admin chose or the ones
//! guessed for them.
//!
//! 1. [`detect`] tells a workbook from a CSV by its first bytes and refuses
//!    an OLE compound file (an .xls, or a password-protected workbook).
//! 2. [`repack`] copies the parts calamine reads into a fresh archive
//!    through `Read::take`: no part unpacks past 64 MB and the whole past
//!    128 MB, and archives with encrypted parts or more than 10,000 entries
//!    are refused. calamine only ever sees the repacked copy.
//! 3. Each worksheet's cells are streamed (never a dense range, which one
//!    far-off cell would blow up) and turned into the text an importer
//!    reads: strings trimmed, whole numbers without `.0`, dates as
//!    `YYYY-MM-DD` (or ISO with the time), formulas as their cached value,
//!    `#N/A` and friends kept as cell errors.
//!
//! Rows keep their sheet row numbers; empty rows are skipped but still
//! counted. The caller runs [`read`] off the async workers, with a
//! timeout, and holds no database connection while it does.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Cursor, Read, Write};

use calamine::{DataRef, ExcelDateTime, Reader, SheetType, SheetVisible, Xlsx};
use chrono::Timelike;

use super::csv_parser::{
    ParsedCsv, SourceRow, MAX_CELL_CHARS, MAX_COLUMNS, MAX_HEADER_CHARS, MAX_ROWS,
};
use super::mapping::{self, ImportSource, PreviewRow, SheetInfo};
use super::safe_cells::strip_formula_guard;

/// Most entries a workbook's zip may hold.
pub const MAX_ENTRIES: usize = 10_000;
/// Most bytes one part may unpack to.
pub const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
/// Most bytes all the parts read may unpack to.
pub const MAX_TOTAL_BYTES: u64 = 128 * 1024 * 1024;
/// Most bytes of cell text kept from one workbook, so shared strings
/// repeated across many cells can't multiply past the unpacked size.
const MAX_TEXT_BYTES: usize = 64 * 1024 * 1024;
/// Cells at or right of this column (0-based) aren't kept; a row holding
/// one is too wide to import.
const MAX_SCAN_COLUMN: u32 = 4 * MAX_COLUMNS as u32;
/// Non-empty rows kept per sheet: the row cap plus room for title rows
/// above the header.
const MAX_SCAN_ROWS: usize = MAX_ROWS + 50;
/// The header row is guessed among this many non-empty rows at the top.
const HEADER_SCAN_ROWS: usize = 10;
/// How many rows below a header candidate it is compared with.
const HEADER_LOOKAHEAD: usize = 5;
/// Rows and columns in a sheet preview.
const PREVIEW_ROWS: usize = 10;
const PREVIEW_COLUMNS: usize = 12;

/// The first bytes of an OLE compound file: an .xls workbook, or any
/// workbook saved with a password.
const CFB_MAGIC: [u8; 4] = [0xD0, 0xCF, 0x11, 0xE0];
const ZIP_MAGIC: [u8; 4] = *b"PK\x03\x04";

/// What an upload is, by its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileFormat {
    Csv,
    Xlsx,
}

impl FileFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Xlsx => "xlsx",
        }
    }
}

/// Why a workbook can't be imported, worded for the admin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

fn refused(message: impl Into<String>) -> Refused {
    Refused(message.into())
}

/// The refusal for an OLE compound file.
pub const PROTECTED_OR_XLS: &str =
    "This file is password-protected or an older .xls workbook. Save it as .xlsx without a password and upload it again.";

fn unreadable() -> Refused {
    refused("This workbook can't be read. Open it in Excel, save it as .xlsx and upload it again.")
}

fn too_large() -> Refused {
    refused("This workbook is too large once unpacked. Copy the sheet you need into a new workbook and upload that.")
}

/// Tell a workbook from a CSV. A zip is taken for a workbook; anything
/// that isn't a zip or an OLE file is read as CSV.
pub fn detect(bytes: &[u8]) -> Result<FileFormat, Refused> {
    if bytes.starts_with(&CFB_MAGIC) {
        return Err(refused(PROTECTED_OR_XLS));
    }
    if bytes.starts_with(&ZIP_MAGIC) {
        return Ok(FileFormat::Xlsx);
    }
    Ok(FileFormat::Csv)
}

/// Whether calamine might read a part. Pictures, embedded files and
/// binary parts are left behind.
fn part_needed(name: &str) -> bool {
    const SKIPPED_EXTENSIONS: &[&str] = &[
        ".bin", ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".tif", ".tiff", ".emf", ".wmf", ".svg",
    ];
    let lower = name.to_ascii_lowercase();
    !(lower.ends_with('/')
        || lower.contains("media/")
        || lower.contains("embeddings/")
        || SKIPPED_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)))
}

/// Copy the parts calamine reads into a new archive, stored uncompressed,
/// reading each through `Read::take` so a part that claims one size and
/// inflates to another stops at the cap.
pub fn repack(bytes: &[u8]) -> Result<Vec<u8>, Refused> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| unreadable())?;
    if archive.len() > MAX_ENTRIES {
        return Err(refused(format!(
            "This workbook holds more than {} parts, so it can't be imported. Copy the sheet you need into a new workbook and upload that.",
            fmt_count(MAX_ENTRIES)
        )));
    }
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(true);
    let mut total: u64 = 0;
    let mut buf = Vec::new();
    let mut names = HashSet::new();
    for i in 0..archive.len() {
        let name = {
            let raw = archive.by_index_raw(i).map_err(|_| unreadable())?;
            if raw.encrypted() {
                return Err(refused(PROTECTED_OR_XLS));
            }
            raw.name().to_string()
        };
        if !part_needed(&name) || !names.insert(name.clone()) {
            continue;
        }
        let entry = archive.by_index(i).map_err(|_| unreadable())?;
        buf.clear();
        entry
            .take(MAX_ENTRY_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|_| unreadable())?;
        let len = buf.len() as u64;
        if len > MAX_ENTRY_BYTES {
            return Err(too_large());
        }
        total += len;
        if total > MAX_TOTAL_BYTES {
            return Err(too_large());
        }
        out.start_file(name, options).map_err(|_| unreadable())?;
        out.write_all(&buf).map_err(|_| unreadable())?;
    }
    Ok(out.finish().map_err(|_| unreadable())?.into_inner())
}

/// The admin's choice of sheet and header row. Without a header row the
/// guess is used.
#[derive(Debug, Clone)]
pub struct SheetChoice {
    pub sheet: String,
    pub header_row: Option<usize>,
}

/// A read workbook: the chosen sheet's grid and how the Map step shows it.
#[derive(Debug)]
pub struct Workbook {
    pub grid: ParsedCsv,
    pub source: ImportSource,
}

/// Read an uploaded workbook into the grid of one sheet: the chosen one,
/// or the first visible sheet with rows that import (a hidden sheet only
/// when no visible one has any).
pub fn read(bytes: &[u8], choice: Option<&SheetChoice>) -> Result<Workbook, Refused> {
    if detect(bytes)? != FileFormat::Xlsx {
        return Err(unreadable());
    }
    let sheets = scan(repack(bytes)?)?;

    let (index, header_row, built) = match choice {
        Some(choice) => {
            let index = sheets
                .iter()
                .position(|s| s.name == choice.sheet)
                .ok_or_else(|| {
                    refused(format!(
                        "This workbook has no sheet named '{}'.",
                        choice.sheet
                    ))
                })?;
            let sheet = &sheets[index];
            let header_row = match choice.header_row {
                Some(row) => u32::try_from(row)
                    .ok()
                    .filter(|r| *r >= 1)
                    .ok_or_else(|| empty_header(sheet, row))?,
                None => guess_header_row(&sheet.rows)
                    .ok_or_else(|| refused(format!("The sheet '{}' is empty.", sheet.name)))?,
            };
            (index, header_row, build_grid(sheet, header_row)?)
        }
        None => default_sheet(&sheets)?,
    };

    let sheet = &sheets[index];
    let infos = sheets
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let header = if i == index {
                Some(header_row)
            } else {
                guess_header_row(&s.rows)
            };
            SheetInfo {
                name: s.name.clone(),
                visible: s.visible,
                rows: header.map_or(0, |h| s.data_rows(h)),
            }
        })
        .collect();
    let preview = preview(sheet, header_row, built.first_column, built.width);
    let grid = built.grid;
    let source = ImportSource {
        format: FileFormat::Xlsx.as_str().to_string(),
        sheets: infos,
        sheet: Some(sheet.name.clone()),
        header_row: header_row as usize,
        headers: mapping::source_headers(&grid),
        row_count: grid.rows.len(),
        delimiter: None,
        preview,
    };
    Ok(Workbook { grid, source })
}

/// The sheet an upload opens on: visible sheets first, in workbook order,
/// then hidden ones; the first with data under its guessed header that
/// builds. When every sheet with data fails, the first failure says why.
fn default_sheet(sheets: &[ScannedSheet]) -> Result<(usize, u32, BuiltGrid), Refused> {
    let order = sheets
        .iter()
        .enumerate()
        .filter(|(_, s)| s.visible)
        .chain(sheets.iter().enumerate().filter(|(_, s)| !s.visible));
    let mut first_error = None;
    for (index, sheet) in order {
        let Some(header_row) = guess_header_row(&sheet.rows) else {
            continue;
        };
        if sheet.data_rows(header_row) == 0 {
            continue;
        }
        match build_grid(sheet, header_row) {
            Ok(built) => return Ok((index, header_row, built)),
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }
    Err(first_error.unwrap_or_else(|| {
        refused(
            "This workbook has no rows to import. Check that the data is on a worksheet and saved.",
        )
    }))
}

fn empty_header(sheet: &ScannedSheet, row: usize) -> Refused {
    refused(format!(
        "Row {row} of '{}' is empty. Choose the row that holds the column headers.",
        sheet.name
    ))
}

// ---- scanning ----

/// How a cell's value reads, for the header-row guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellKind {
    Text,
    /// A number, date, time or true/false.
    Value,
    /// A spreadsheet error such as `#N/A`; the text is the error.
    Error,
    /// Text longer than a cell may hold; the text isn't kept.
    TooLong,
}

#[derive(Debug, Clone)]
struct ScanCell {
    /// 0-based column.
    col: u32,
    value: String,
    kind: CellKind,
}

#[derive(Debug, Default)]
struct ScanRow {
    /// Non-empty cells, left to right.
    cells: Vec<ScanCell>,
    /// The row has a cell too far right to keep.
    overflow: bool,
}

impl ScanRow {
    fn filled(&self) -> usize {
        self.cells.len() + usize::from(self.overflow)
    }

    fn text(&self) -> usize {
        self.cells
            .iter()
            .filter(|c| matches!(c.kind, CellKind::Text | CellKind::TooLong))
            .count()
    }
}

#[derive(Debug)]
struct ScannedSheet {
    name: String,
    visible: bool,
    /// Non-empty rows by 1-based sheet row number.
    rows: BTreeMap<u32, ScanRow>,
    /// The sheet has more non-empty rows than were kept.
    truncated: bool,
}

impl ScannedSheet {
    /// Non-empty rows under `header_row`, one more than the cap when the
    /// sheet has more.
    fn data_rows(&self, header_row: u32) -> usize {
        let count = self.rows.range(header_row.saturating_add(1)..).count();
        if self.truncated {
            count.max(MAX_ROWS + 1)
        } else {
            count
        }
    }
}

/// Stream every worksheet of a repacked workbook into its non-empty rows.
fn scan(repacked: Vec<u8>) -> Result<Vec<ScannedSheet>, Refused> {
    let mut workbook: Xlsx<_> = Xlsx::new(Cursor::new(repacked)).map_err(|e| {
        tracing::debug!(error = %e, "workbook refused by the reader");
        unreadable()
    })?;
    let metas: Vec<calamine::Sheet> = workbook.sheets_metadata().to_vec();
    let mut budget = MAX_TEXT_BYTES;
    let mut sheets = Vec::new();
    for meta in metas {
        if meta.typ != SheetType::WorkSheet {
            continue;
        }
        let mut sheet = ScannedSheet {
            name: meta.name.clone(),
            visible: meta.visible == SheetVisible::Visible,
            rows: BTreeMap::new(),
            truncated: false,
        };
        let mut cells = workbook
            .worksheet_cells_reader(&meta.name)
            .map_err(|_| unreadable())?;
        while let Some(cell) = cells.next_cell().map_err(|_| unreadable())? {
            let Some((value, kind)) = canonical(cell.get_value()) else {
                continue;
            };
            let (row, col) = cell.get_position();
            let row = row.saturating_add(1);
            if !sheet.rows.contains_key(&row) && sheet.rows.len() >= MAX_SCAN_ROWS {
                sheet.truncated = true;
                break;
            }
            let entry = sheet.rows.entry(row).or_default();
            if col >= MAX_SCAN_COLUMN {
                entry.overflow = true;
                continue;
            }
            budget = budget.checked_sub(value.len()).ok_or_else(too_large)?;
            entry.cells.push(ScanCell { col, value, kind });
        }
        for row in sheet.rows.values_mut() {
            // A cell written twice keeps its last value.
            row.cells.reverse();
            row.cells.sort_by_key(|c| c.col);
            row.cells.dedup_by_key(|c| c.col);
        }
        sheets.push(sheet);
    }
    Ok(sheets)
}

/// The text an importer reads for a cell, and how it reads; `None` for an
/// empty cell.
fn canonical(value: &DataRef<'_>) -> Option<(String, CellKind)> {
    let typed = |text: String| Some((text, CellKind::Value));
    match value {
        DataRef::Empty => None,
        DataRef::String(s) => text_cell(s),
        DataRef::SharedString(s) => text_cell(s),
        DataRef::Int(i) => typed(i.to_string()),
        DataRef::Float(f) => typed(format_number(*f)),
        DataRef::Bool(b) => typed(b.to_string()),
        DataRef::DateTime(dt) => typed(format_datetime(dt)),
        DataRef::DateTimeIso(s) => typed(
            s.strip_suffix("T00:00:00")
                .unwrap_or(s.as_str())
                .to_string(),
        ),
        DataRef::DurationIso(s) => typed(s.clone()),
        DataRef::Error(e) => Some((e.to_string(), CellKind::Error)),
    }
}

fn text_cell(raw: &str) -> Option<(String, CellKind)> {
    let text = strip_formula_guard(raw.trim()).trim();
    if text.is_empty() {
        return None;
    }
    if text.chars().count() > MAX_CELL_CHARS {
        return Some((String::new(), CellKind::TooLong));
    }
    Some((text.to_string(), CellKind::Text))
}

/// A number as Excel shows it in General format: whole numbers without
/// `.0`, others at Excel's 15 significant digits, never in exponent form.
pub fn format_number(f: f64) -> String {
    if !f.is_finite() {
        return f.to_string();
    }
    let whole = |v: f64| v.fract() == 0.0 && v.abs() < 1e15;
    if whole(f) {
        return format!("{}", f as i64);
    }
    let rounded: f64 = format!("{f:.14e}").parse().unwrap_or(f);
    if whole(rounded) {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded}")
    }
}

/// A date cell as ISO text: `YYYY-MM-DD` at midnight, else with the time
/// (`YYYY-MM-DDTHH:MM:SS`); a time alone as `HH:MM:SS`; a duration as
/// `H:MM:SS`. Rounded to the second.
fn format_datetime(dt: &ExcelDateTime) -> String {
    let value = dt.as_f64();
    if dt.is_duration() {
        let secs = (value * 86_400.0).round() as i64;
        let sign = if secs < 0 { "-" } else { "" };
        let secs = secs.abs();
        return format!(
            "{sign}{}:{:02}:{:02}",
            secs / 3600,
            (secs / 60) % 60,
            secs % 60
        );
    }
    if (0.0..1.0).contains(&value) {
        let secs = ((value * 86_400.0).round() as i64).rem_euclid(86_400);
        return format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            (secs / 60) % 60,
            secs % 60
        );
    }
    let (y, mo, d, h, mi, s, ms) = dt.to_ymd_hms_milli();
    let parsed = chrono::NaiveDate::from_ymd_opt(i32::from(y), u32::from(mo), u32::from(d))
        .and_then(|date| {
            date.and_hms_milli_opt(u32::from(h), u32::from(mi), u32::from(s), u32::from(ms))
        });
    let Some(at) = parsed else {
        return format_number(value);
    };
    let at = at + chrono::Duration::milliseconds(500);
    let at = at.with_nanosecond(0).unwrap_or(at);
    if at.time() == chrono::NaiveTime::MIN {
        at.format("%Y-%m-%d").to_string()
    } else {
        at.format("%Y-%m-%dT%H:%M:%S").to_string()
    }
}

// ---- header row ----

/// The header row: among the first non-empty rows, the first that is
/// mostly text and at least as full as the rows under it (a title row
/// above the header has one cell; the header has one per column). When no
/// row fits, the first non-empty row.
fn guess_header_row(rows: &BTreeMap<u32, ScanRow>) -> Option<u32> {
    let filled: Vec<(u32, usize, usize)> = rows
        .iter()
        .map(|(&row, r)| (row, r.filled(), r.text()))
        .collect();
    for (i, &(row, cells, text)) in filled.iter().take(HEADER_SCAN_ROWS).enumerate() {
        if cells == 0 || text * 2 < cells {
            continue;
        }
        let mut below: Vec<usize> = filled
            .iter()
            .skip(i + 1)
            .take(HEADER_LOOKAHEAD)
            .map(|&(_, n, _)| n)
            .collect();
        if below.is_empty() {
            return Some(row);
        }
        below.sort_unstable();
        if cells >= below[below.len() / 2] {
            return Some(row);
        }
    }
    filled.first().map(|&(row, _, _)| row)
}

// ---- grid ----

struct BuiltGrid {
    grid: ParsedCsv,
    /// 0-based sheet column of the grid's first column.
    first_column: u32,
    /// Sheet columns the grid spans, before empty unnamed ones are dropped.
    width: usize,
}

/// Column letters for a 0-based column: 0 is A, 26 is AA.
pub fn column_letter(col: u32) -> String {
    let mut n = u64::from(col) + 1;
    let mut out = Vec::new();
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        out.push(char::from(b'A' + rem));
        n = (n - 1) / 26;
    }
    out.iter().rev().collect()
}

/// The grid under `header_row`: one column per sheet column from the
/// leftmost to the rightmost used one, less columns with neither a header
/// nor a value. A column with values but no header is named by its letter;
/// a repeated header gets its letter added.
fn build_grid(sheet: &ScannedSheet, header_row: u32) -> Result<BuiltGrid, Refused> {
    let header = sheet
        .rows
        .get(&header_row)
        .filter(|r| r.filled() > 0)
        .ok_or_else(|| empty_header(sheet, header_row as usize))?;
    let data: Vec<(u32, &ScanRow)> = sheet
        .rows
        .range(header_row.saturating_add(1)..)
        .map(|(&row, r)| (row, r))
        .collect();
    if sheet.truncated || data.len() > MAX_ROWS {
        return Err(refused(format!(
            "The sheet '{}' has more than {} rows under its header. Split it into smaller sheets and import them one at a time.",
            sheet.name,
            fmt_count(MAX_ROWS)
        )));
    }
    if data.is_empty() {
        return Err(refused(format!(
            "There are no rows under the header row (row {header_row}) of '{}'.",
            sheet.name
        )));
    }
    let too_wide = |width: usize| {
        refused(format!(
            "The sheet '{}' has {width} columns from the header row down; an import takes at most {MAX_COLUMNS}.",
            sheet.name
        ))
    };
    if header.overflow || data.iter().any(|(_, r)| r.overflow) {
        return Err(too_wide(MAX_SCAN_COLUMN as usize));
    }

    let cells = || {
        header
            .cells
            .iter()
            .chain(data.iter().flat_map(|(_, r)| r.cells.iter()))
    };
    let first = cells().map(|c| c.col).min().unwrap_or(0);
    let last = cells().map(|c| c.col).max().unwrap_or(first);
    let width = (last - first) as usize + 1;
    if width > MAX_COLUMNS {
        return Err(too_wide(width));
    }

    let mut header_text = vec![String::new(); width];
    for cell in &header.cells {
        let at = (cell.col - first) as usize;
        if cell.kind == CellKind::TooLong || cell.value.chars().count() > MAX_HEADER_CHARS {
            return Err(refused(format!(
                "The header in column {} is longer than {MAX_HEADER_CHARS} characters.",
                column_letter(cell.col)
            )));
        }
        header_text[at] = cell.value.clone();
    }
    let mut has_values = vec![false; width];
    for (_, row) in &data {
        for cell in &row.cells {
            has_values[(cell.col - first) as usize] = true;
        }
    }

    // Sheet column offset to grid column index.
    let mut slot: Vec<Option<usize>> = vec![None; width];
    let mut headers = Vec::new();
    let mut seen = HashSet::new();
    for at in 0..width {
        if header_text[at].is_empty() && !has_values[at] {
            continue;
        }
        let letter = column_letter(first + at as u32);
        let mut name = if header_text[at].is_empty() {
            format!("Column {letter}")
        } else {
            header_text[at].clone()
        };
        if !seen.insert(name.clone()) {
            name = format!("{name} ({letter})");
            seen.insert(name.clone());
        }
        slot[at] = Some(headers.len());
        headers.push(name);
    }

    let mut rows = Vec::with_capacity(data.len());
    for (row_number, row) in data {
        let mut cells = vec![String::new(); headers.len()];
        let mut cell_errors = Vec::new();
        for cell in &row.cells {
            let Some(index) = slot[(cell.col - first) as usize] else {
                continue;
            };
            if cell.kind == CellKind::TooLong {
                return Err(refused(format!(
                    "Row {row_number}, column '{}' is longer than {} characters.",
                    headers[index],
                    fmt_count(MAX_CELL_CHARS)
                )));
            }
            if cell.kind == CellKind::Error {
                cell_errors.push(index);
            }
            cells[index] = cell.value.clone();
        }
        rows.push(SourceRow {
            source_row: row_number as usize,
            cells,
            cell_errors,
        });
    }

    Ok(BuiltGrid {
        grid: ParsedCsv {
            headers,
            rows,
            delimiter: b',',
        },
        first_column: first,
        width,
    })
}

/// The top of the sheet around the header row, blank rows included, from
/// the grid's first column.
fn preview(sheet: &ScannedSheet, header_row: u32, first: u32, width: usize) -> Vec<PreviewRow> {
    let header_row = header_row as usize;
    let start = if header_row + 3 <= PREVIEW_ROWS {
        1
    } else {
        header_row - 3
    };
    let last = sheet.rows.keys().next_back().map_or(0, |&r| r as usize);
    let end = (start + PREVIEW_ROWS - 1).min(last);
    let columns = width.min(PREVIEW_COLUMNS);
    let index: HashMap<u32, &ScanRow> = sheet
        .rows
        .range(start as u32..=end as u32)
        .map(|(&r, row)| (r, row))
        .collect();
    (start..=end)
        .map(|row| {
            let mut cells = vec![String::new(); columns];
            if let Some(scanned) = index.get(&(row as u32)) {
                for cell in &scanned.cells {
                    if cell.col < first {
                        continue;
                    }
                    let at = (cell.col - first) as usize;
                    if at < columns {
                        cells[at] = mapping::shorten(&cell.value);
                    }
                }
            }
            PreviewRow { row, cells }
        })
        .collect()
}

/// A count with thousands separators: 5000 as "5,000".
fn fmt_count(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::ExcelDateTimeType;
    use rust_xlsxwriter::{ExcelDateTime as XlDate, Format, Formula, Workbook as XlWorkbook};

    fn scan_row(cells: &[(u32, &str, CellKind)]) -> ScanRow {
        ScanRow {
            cells: cells
                .iter()
                .map(|&(col, value, kind)| ScanCell {
                    col,
                    value: value.to_string(),
                    kind,
                })
                .collect(),
            overflow: false,
        }
    }

    fn texts(n: u32) -> Vec<(u32, &'static str, CellKind)> {
        (0..n).map(|c| (c, "x", CellKind::Text)).collect()
    }

    fn values(n: u32) -> Vec<(u32, &'static str, CellKind)> {
        (0..n).map(|c| (c, "1", CellKind::Value)).collect()
    }

    fn rows(spec: &[(u32, Vec<(u32, &'static str, CellKind)>)]) -> BTreeMap<u32, ScanRow> {
        spec.iter()
            .map(|(row, cells)| (*row, scan_row(cells)))
            .collect()
    }

    #[test]
    fn canonicalises_cell_values() {
        let text = |v: DataRef<'_>| canonical(&v).map(|(s, _)| s);
        assert_eq!(text(DataRef::Empty), None);
        assert_eq!(
            text(DataRef::String("  Camera ".into())),
            Some("Camera".into())
        );
        assert_eq!(text(DataRef::SharedString("   ")), None);
        assert_eq!(
            text(DataRef::SharedString("'=SUM(A1)")),
            Some("=SUM(A1)".into())
        );
        assert_eq!(text(DataRef::Int(42)), Some("42".into()));
        assert_eq!(text(DataRef::Float(3.0)), Some("3".into()));
        assert_eq!(text(DataRef::Float(-12.0)), Some("-12".into()));
        assert_eq!(text(DataRef::Float(7450.5)), Some("7450.5".into()));
        assert_eq!(text(DataRef::Float(0.1 + 0.2)), Some("0.3".into()));
        assert_eq!(
            text(DataRef::Float(1e20)),
            Some("100000000000000000000".into())
        );
        assert_eq!(text(DataRef::Bool(true)), Some("true".into()));
        assert_eq!(
            canonical(&DataRef::Error(calamine::CellErrorType::NA)),
            Some(("#N/A".into(), CellKind::Error))
        );
        assert_eq!(
            text(DataRef::DateTimeIso("2024-03-05T00:00:00".into())),
            Some("2024-03-05".into())
        );
    }

    #[test]
    fn dates_read_as_iso() {
        let date =
            |v: f64| format_datetime(&ExcelDateTime::new(v, ExcelDateTimeType::DateTime, false));
        // 45356 is 2024-03-05.
        assert_eq!(date(45356.0), "2024-03-05");
        assert_eq!(date(45356.5), "2024-03-05T12:00:00");
        // A hair under midnight rounds to the date.
        assert_eq!(date(45_356.999_999_9), "2024-03-06");
        assert_eq!(date(0.75), "18:00:00");
        let duration = ExcelDateTime::new(1.5, ExcelDateTimeType::TimeDelta, false);
        assert_eq!(format_datetime(&duration), "36:00:00");
        // The 1904 date system.
        let mac = ExcelDateTime::new(43894.0, ExcelDateTimeType::DateTime, true);
        assert_eq!(format_datetime(&mac), "2024-03-05");
    }

    #[test]
    fn header_guess_skips_title_rows() {
        let sheet = rows(&[
            (1, vec![(0, "TVS Assets 2026", CellKind::Text)]),
            (3, texts(6)),
            (4, texts(6)),
            (5, texts(5)),
            (6, texts(6)),
        ]);
        assert_eq!(guess_header_row(&sheet), Some(3));
    }

    #[test]
    fn header_guess_takes_row_one_when_it_fits() {
        let sheet = rows(&[(1, texts(4)), (2, texts(4)), (3, texts(3))]);
        assert_eq!(guess_header_row(&sheet), Some(1));
        // A data row one cell wider than the header doesn't push it out.
        let sheet = rows(&[(1, texts(4)), (2, texts(5)), (3, texts(4)), (4, texts(4))]);
        assert_eq!(guess_header_row(&sheet), Some(1));
    }

    #[test]
    fn header_guess_prefers_a_mostly_text_row() {
        // A row of numbers above the real header is not the header.
        let sheet = rows(&[
            (1, values(5)),
            (2, texts(5)),
            (3, values(5)),
            (4, values(5)),
        ]);
        assert_eq!(guess_header_row(&sheet), Some(2));
    }

    #[test]
    fn header_guess_falls_back_to_the_first_row() {
        let sheet = rows(&[(2, values(3)), (3, values(3))]);
        assert_eq!(guess_header_row(&sheet), Some(2));
        assert_eq!(guess_header_row(&BTreeMap::new()), None);
    }

    #[test]
    fn column_letters() {
        assert_eq!(column_letter(0), "A");
        assert_eq!(column_letter(25), "Z");
        assert_eq!(column_letter(26), "AA");
        assert_eq!(column_letter(701), "ZZ");
        assert_eq!(column_letter(702), "AAA");
    }

    #[test]
    fn counts_have_separators() {
        assert_eq!(fmt_count(5_000), "5,000");
        assert_eq!(fmt_count(10_000), "10,000");
        assert_eq!(fmt_count(999), "999");
    }

    #[test]
    fn detects_by_magic_bytes() {
        assert_eq!(detect(b"PK\x03\x04rest"), Ok(FileFormat::Xlsx));
        assert_eq!(detect(b"name,kind\n"), Ok(FileFormat::Csv));
        assert_eq!(
            detect(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1]),
            Err(Refused(PROTECTED_OR_XLS.to_string()))
        );
    }

    /// A workbook with a title, a blank row, the header on row 3, a gap in
    /// the data, a date, a whole float, a formula and a hidden sheet.
    fn sample_workbook() -> Vec<u8> {
        let mut wb = XlWorkbook::new();
        let date = Format::new().set_num_format("yyyy-mm-dd");
        {
            let ws = wb.add_worksheet();
            ws.set_name("Cameras").expect("name");
            ws.write_string(0, 0, "TVS Assets 2026").expect("write");
            for (c, h) in ["Asset #", "Name", "Qty", "Purchased", "Total", "Name"]
                .iter()
                .enumerate()
            {
                ws.write_string(2, c as u16, *h).expect("write");
            }
            ws.write_string(3, 0, "TVS-0001").expect("write");
            ws.write_string(3, 1, " Camera A ").expect("write");
            ws.write_number(3, 2, 2.0).expect("write");
            ws.write_datetime_with_format(3, 3, XlDate::from_ymd(2024, 3, 5).expect("date"), &date)
                .expect("write");
            ws.write_formula(3, 4, Formula::new("=C4*2").set_result("4"))
                .expect("write");
            ws.write_string(3, 5, "dup").expect("write");
            // Row 5 left empty; row 6 carries on.
            ws.write_string(5, 0, "TVS-0002").expect("write");
            ws.write_string(5, 1, "Camera B").expect("write");
            ws.write_number(5, 2, 1.5).expect("write");
            ws.write_formula(5, 3, Formula::new("=NA()").set_result("#N/A"))
                .expect("write");
            ws.write_string(5, 7, "stray").expect("write");
        }
        {
            let ws = wb.add_worksheet();
            ws.set_name("Lookups").expect("name");
            ws.set_hidden(true);
            ws.write_string(0, 0, "Kind").expect("write");
            ws.write_string(1, 0, "Camera").expect("write");
        }
        wb.save_to_buffer().expect("save")
    }

    #[test]
    fn reads_a_workbook_with_true_row_numbers() {
        let read = read(&sample_workbook(), None).expect("read");
        let source = &read.source;
        assert_eq!(source.format, "xlsx");
        assert_eq!(source.sheet.as_deref(), Some("Cameras"));
        assert_eq!(source.header_row, 3);
        let sheets: Vec<(&str, bool, usize)> = source
            .sheets
            .iter()
            .map(|s| (s.name.as_str(), s.visible, s.rows))
            .collect();
        assert_eq!(sheets, vec![("Cameras", true, 2), ("Lookups", false, 1)]);

        let grid = &read.grid;
        assert_eq!(
            grid.headers,
            vec![
                "Asset #",
                "Name",
                "Qty",
                "Purchased",
                "Total",
                "Name (F)",
                "Column H"
            ]
        );
        let numbers: Vec<usize> = grid.rows.iter().map(|r| r.source_row).collect();
        assert_eq!(numbers, vec![4, 6]);
        assert_eq!(
            grid.rows[0].cells,
            vec!["TVS-0001", "Camera A", "2", "2024-03-05", "4", "dup", ""]
        );
        assert_eq!(grid.rows[1].cells[2], "1.5");
        assert_eq!(grid.rows[1].cells[6], "stray");
        assert!(grid.rows[0].cell_errors.is_empty());
        assert_eq!(grid.rows[1].cells[3], "#N/A");
        assert_eq!(grid.rows[1].cell_errors, vec![3]);
        // A cell error is no sample.
        assert_eq!(source.headers[3].samples, vec!["2024-03-05"]);

        let preview_rows: Vec<usize> = source.preview.iter().map(|p| p.row).collect();
        assert_eq!(preview_rows, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(source.preview[0].cells[0], "TVS Assets 2026");
        assert_eq!(source.preview[2].cells[1], "Name");
    }

    #[test]
    fn a_chosen_sheet_and_header_row_are_read() {
        let bytes = sample_workbook();
        let read = read(
            &bytes,
            Some(&SheetChoice {
                sheet: "Lookups".into(),
                header_row: None,
            }),
        )
        .expect("read");
        assert_eq!(read.source.sheet.as_deref(), Some("Lookups"));
        assert_eq!(read.grid.headers, vec!["Kind"]);
        assert_eq!(read.grid.rows[0].source_row, 2);

        // The title row as header: everything under it is data.
        let read = super::read(
            &bytes,
            Some(&SheetChoice {
                sheet: "Cameras".into(),
                header_row: Some(1),
            }),
        )
        .expect("read");
        assert_eq!(read.source.header_row, 1);
        assert_eq!(read.grid.rows.len(), 3);

        let err = super::read(
            &bytes,
            Some(&SheetChoice {
                sheet: "Cameras".into(),
                header_row: Some(2),
            }),
        )
        .unwrap_err();
        assert!(err.0.contains("Row 2 of 'Cameras' is empty"), "{err}");
        let err = super::read(
            &bytes,
            Some(&SheetChoice {
                sheet: "Nope".into(),
                header_row: None,
            }),
        )
        .unwrap_err();
        assert!(err.0.contains("no sheet named 'Nope'"), "{err}");
    }

    #[test]
    fn repack_refuses_too_many_entries() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for i in 0..=MAX_ENTRIES {
            zip.start_file(format!("f{i}.xml"), options).expect("start");
        }
        let bytes = zip.finish().expect("finish").into_inner();
        let err = repack(&bytes).unwrap_err();
        assert!(err.0.contains("more than 10,000 parts"), "{err}");
    }
}
