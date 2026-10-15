//! Column mapping: what the uploaded file looks like (`ImportSource`),
//! which field each column feeds (`ColumnMapping`), suggestions for that
//! mapping, and the mapped grid the importers read.
//!
//! The importers stay unaware of the file's own headers: `apply_mapping`
//! hands them a grid with one column per field id, blank where nothing
//! maps, so the same importer serves a template upload and a spreadsheet
//! with headers of its own.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::csv_parser::{ParsedCsv, SourceRow, MAX_CELL_CHARS, MAX_ROWS};
use super::types::{
    ApplyTo, DecimalMark, FieldSpec, ImportOptions, ImportSummary, ImportType, RowOutcome,
    RowResult, ValueAnswer, ValueAnswers,
};

/// Sample values carried per header.
pub const SAMPLES: usize = 3;
/// Longest sample value, in characters.
const SAMPLE_CHARS: usize = 100;

/// Headers of the error report's own columns, normalized. A re-uploaded
/// report maps them to nothing.
const REPORT_HEADERS: &[&str] = &["row", "errors"];

/// Whether `header` is one of the error report's own columns.
pub fn is_report_header(header: &str) -> bool {
    REPORT_HEADERS.contains(&normalize(header).as_str())
}

/// Field id to the 0-based index of the column that feeds it.
pub type ColumnMapping = BTreeMap<String, usize>;

/// The uploaded file as the Map step shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSource {
    /// `csv` or `xlsx`.
    pub format: String,
    /// Workbook sheets in workbook order; empty for CSV.
    #[serde(default)]
    pub sheets: Vec<SheetInfo>,
    /// The workbook sheet the grid was read from.
    pub sheet: Option<String>,
    /// 1-based line (CSV) or sheet row (xlsx) of the header row.
    pub header_row: usize,
    pub headers: Vec<SourceHeader>,
    pub row_count: usize,
    /// The CSV field separator, so the client can default the decimal mark.
    #[serde(default)]
    pub delimiter: Option<String>,
    /// The top of the chosen sheet as it reads, title rows included, so the
    /// admin can confirm the header row. Empty for CSV.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preview: Vec<PreviewRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SheetInfo {
    pub name: String,
    /// False for a hidden or very hidden sheet.
    pub visible: bool,
    /// Data rows under the sheet's header row (its guessed one, or the
    /// chosen one for the chosen sheet). A sheet with more rows than an
    /// import takes reports one more than the cap.
    pub rows: usize,
}

/// One row of a sheet preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewRow {
    /// The sheet row number, 1-based.
    pub row: usize,
    /// The row's cells from the grid's first column, shortened.
    pub cells: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHeader {
    pub index: usize,
    pub name: String,
    /// The first non-blank values in the column, at most [`SAMPLES`].
    pub samples: Vec<String>,
}

/// Why a column was pre-mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionReason {
    /// The same header went this way, to a field or to Don't import, in the
    /// type's last mapped import: a saved match.
    Previous,
    /// The header is the field id.
    Exact,
    /// The header is the field's label.
    Label,
    /// The header is a known other name for the field.
    Synonym,
    /// An error report's own column (`Row`, `Errors`): imports nothing.
    Report,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub index: usize,
    /// `None` for a column that should not be imported.
    pub field: Option<String>,
    pub reason: SuggestionReason,
    /// For a saved match, the import it came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
}

/// The import a saved column match came from: who mapped it and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub job_id: Uuid,
    /// The admin who started that import.
    pub by: Option<Uuid>,
    pub by_name: Option<String>,
    /// When that import's mapping was last stored or applied.
    pub at: chrono::DateTime<chrono::Utc>,
}

/// What the type's last mapped import says about each header: normalized
/// header to field id, or to `None` for a column set to Don't import. A
/// header that import left without a choice is absent.
#[derive(Debug, Clone, Default)]
pub struct HeaderMemory {
    pub choices: HashMap<String, Option<String>>,
    pub provenance: Option<Provenance>,
}

/// What the `source` column stores: the source plus the suggestions made
/// at upload, so a resumed job shows the same suggestions, and the columns
/// the admin set to Don't import at the Map step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSource {
    #[serde(flatten)]
    pub source: ImportSource,
    #[serde(default)]
    pub suggested_mapping: Vec<Suggestion>,
    /// Column indexes set to Don't import, stored with the mapping. A
    /// column neither mapped nor here was left without a choice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignored: Vec<usize>,
}

/// Lowercase letters and digits only, with `#` read as "no" so `Asset #`
/// and `Serial #` meet `asset no` and `serial no`.
pub fn normalize(header: &str) -> String {
    let mut out = String::with_capacity(header.len());
    for c in header.chars() {
        if c == '#' {
            out.push_str("no");
        } else if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// A grid's headers, each with its first non-blank values. A cell holding
/// a spreadsheet error is no sample.
pub fn source_headers(parsed: &ParsedCsv) -> Vec<SourceHeader> {
    parsed
        .headers
        .iter()
        .enumerate()
        .map(|(index, name)| SourceHeader {
            index,
            name: name.clone(),
            samples: parsed
                .rows
                .iter()
                .filter(|r| !r.cell_errors.contains(&index))
                .filter_map(|r| r.cells.get(index).map(String::as_str))
                .filter(|v| !v.is_empty())
                .take(SAMPLES)
                .map(shorten)
                .collect(),
        })
        .collect()
}

/// A value cut to [`SAMPLE_CHARS`] characters.
pub fn shorten(value: &str) -> String {
    value.chars().take(SAMPLE_CHARS).collect()
}

/// Describe a parsed CSV for the Map step.
pub fn csv_source(parsed: &ParsedCsv) -> ImportSource {
    let headers = source_headers(parsed);
    let delimiter = match parsed.delimiter {
        b';' => ";",
        b'\t' => "\t",
        _ => ",",
    };
    ImportSource {
        format: "csv".to_string(),
        sheets: Vec::new(),
        sheet: None,
        header_row: 1,
        headers,
        row_count: parsed.rows.len(),
        delimiter: Some(delimiter.to_string()),
        preview: Vec::new(),
    }
}

/// What a previous job of the same type chose for each header: its
/// mapping, and its columns set to Don't import.
pub fn previous_choices(
    source: &ImportSource,
    mapping: &ColumnMapping,
    ignored: &[usize],
) -> HashMap<String, Option<String>> {
    let header = |index: usize| source.headers.get(index).map(|h| normalize(&h.name));
    let ignored = ignored
        .iter()
        .filter_map(|&index| Some((header(index)?, None)));
    let mapped = mapping
        .iter()
        .filter_map(|(field, &index)| Some((header(index)?, Some(field.clone()))));
    // A mapped header wins over an ignored one of the same spelling.
    ignored.chain(mapped).collect()
}

/// Pre-map each header. Each field goes to one column at most: the
/// strongest reason wins (a saved match, then exact, label, synonym), then
/// file order. A saved Don't import claims its column like a field would.
/// A blank header gets no suggestion.
pub fn suggest(
    headers: &[SourceHeader],
    fields: &[FieldSpec],
    synonyms: &[(&str, &str)],
    memory: &HeaderMemory,
) -> Vec<Suggestion> {
    let known: HashSet<&str> = fields.iter().map(|f| f.id.as_str()).collect();
    let mut candidates: Vec<(SuggestionReason, usize, Option<String>)> = Vec::new();
    let mut report = Vec::new();
    for header in headers {
        let raw = header.name.trim();
        let norm = normalize(raw);
        if norm.is_empty() {
            continue;
        }
        if REPORT_HEADERS.contains(&norm.as_str()) {
            report.push(header.index);
            continue;
        }
        match memory.choices.get(&norm) {
            Some(None) => candidates.push((SuggestionReason::Previous, header.index, None)),
            Some(Some(field)) if known.contains(field.as_str()) => {
                candidates.push((
                    SuggestionReason::Previous,
                    header.index,
                    Some(field.clone()),
                ));
            }
            // Not chosen last time, or a field that's gone: guess afresh.
            _ => {}
        }
        if let Some(field) = fields.iter().find(|f| f.id == raw) {
            candidates.push((
                SuggestionReason::Exact,
                header.index,
                Some(field.id.clone()),
            ));
        }
        if let Some(field) = fields
            .iter()
            .find(|f| normalize(&f.label) == norm || normalize(&f.id) == norm)
        {
            candidates.push((
                SuggestionReason::Label,
                header.index,
                Some(field.id.clone()),
            ));
        }
        if let Some((_, field)) = synonyms
            .iter()
            .find(|(s, f)| *s == norm && known.contains(f))
        {
            candidates.push((
                SuggestionReason::Synonym,
                header.index,
                Some(field.to_string()),
            ));
        }
    }
    candidates.sort_by_key(|(reason, index, _)| (*reason, *index));

    let mut taken_fields = HashSet::new();
    let mut taken_columns = HashSet::new();
    let mut out: Vec<Suggestion> = report
        .into_iter()
        .map(|index| Suggestion {
            index,
            field: None,
            reason: SuggestionReason::Report,
            provenance: None,
        })
        .collect();
    for (reason, index, field) in candidates {
        if taken_columns.contains(&index)
            || field.as_ref().is_some_and(|f| taken_fields.contains(f))
        {
            continue;
        }
        taken_columns.insert(index);
        if let Some(f) = &field {
            taken_fields.insert(f.clone());
        }
        let provenance = (reason == SuggestionReason::Previous)
            .then(|| memory.provenance.clone())
            .flatten();
        out.push(Suggestion {
            index,
            field,
            reason,
            provenance,
        });
    }
    out.sort_by_key(|s| s.index);
    out
}

/// Check the columns set to Don't import: each in the file once, and none
/// also mapped to a field.
pub fn validate_ignored(
    ignored: &[usize],
    mapping: &ColumnMapping,
    headers: &[SourceHeader],
) -> Result<(), MappingError> {
    let mut seen = HashSet::new();
    for &index in ignored {
        let Some(header) = headers.get(index) else {
            return Err(MappingError(format!(
                "column {} is set to Don't import, but the file has {} columns",
                index + 1,
                headers.len()
            )));
        };
        if !seen.insert(index) {
            return Err(MappingError(format!(
                "column '{}' is set to Don't import twice",
                header.name
            )));
        }
        if mapping.values().any(|&i| i == index) {
            return Err(MappingError(format!(
                "column '{}' is mapped to a field and also set to Don't import; choose one",
                header.name
            )));
        }
    }
    Ok(())
}

/// Why a mapping can't be used, worded for the admin.
#[derive(Debug, PartialEq, Eq)]
pub struct MappingError(pub String);

/// Check a mapping against the fields and the file: every field id known,
/// every column in range and used once, every required field covered.
/// The asset kind may come from `options.default_kind` instead of a
/// column; the caller checks that kind exists.
pub fn validate(
    import_type: ImportType,
    mapping: &ColumnMapping,
    options: &ImportOptions,
    fields: &[FieldSpec],
    headers: &[SourceHeader],
) -> Result<(), MappingError> {
    let header_name = |index: usize| {
        headers
            .get(index)
            .map(|h| h.name.clone())
            .unwrap_or_default()
    };
    let mut by_column: HashMap<usize, &str> = HashMap::new();
    for (field, &index) in mapping {
        let Some(spec) = fields.iter().find(|f| &f.id == field) else {
            return Err(MappingError(format!("unknown field '{field}'")));
        };
        if index >= headers.len() {
            return Err(MappingError(format!(
                "{} is mapped to column {}, but the file has {} columns",
                spec.label,
                index + 1,
                headers.len()
            )));
        }
        if let Some(other) = by_column.insert(index, field) {
            let label = |id: &str| {
                fields
                    .iter()
                    .find(|f| f.id == id)
                    .map_or(id.to_string(), |f| f.label.clone())
            };
            let (a, b) = if other < field.as_str() {
                (other, field.as_str())
            } else {
                (field.as_str(), other)
            };
            return Err(MappingError(format!(
                "column '{}' is mapped to both {} and {}; a column feeds one field",
                header_name(index),
                label(a),
                label(b)
            )));
        }
    }
    validate_constants(import_type, mapping, options, fields)?;
    for spec in fields.iter().filter(|f| f.required) {
        if mapping.contains_key(&spec.id) || options.constants.iter().any(|c| c.field == spec.id) {
            continue;
        }
        let defaulted = import_type == ImportType::Assets
            && spec.id == "kind"
            && options
                .default_kind
                .as_deref()
                .is_some_and(|k| !k.trim().is_empty());
        if !defaulted {
            return Err(MappingError(if spec.id == "kind" {
                "no column maps to Kind; map one or choose a kind for every row".to_string()
            } else {
                format!("no column maps to {}", spec.label)
            }));
        }
    }
    Ok(())
}

/// Fields a constant can't set: the keys rows are matched on, where one
/// value on every row would make every row the same record.
fn key_fields(import_type: ImportType) -> &'static [&'static str] {
    match import_type {
        ImportType::Assets => &["asset_tag", "serial_number"],
        ImportType::Users => &["email"],
        ImportType::Tickets => &[],
    }
}

/// Check the constants: each sets a known field once, one no column feeds
/// and no other option sets, to a value that isn't blank.
fn validate_constants(
    import_type: ImportType,
    mapping: &ColumnMapping,
    options: &ImportOptions,
    fields: &[FieldSpec],
) -> Result<(), MappingError> {
    let mut seen: HashSet<&str> = HashSet::new();
    for constant in &options.constants {
        let field = constant.field.as_str();
        let Some(spec) = fields.iter().find(|f| f.id == field) else {
            return Err(MappingError(format!("unknown field '{field}'")));
        };
        if !seen.insert(field) {
            return Err(MappingError(format!(
                "{} is set for every row twice; set it once",
                spec.label
            )));
        }
        if import_type == ImportType::Users && constant.apply_to == ApplyTo::All {
            return Err(MappingError(format!(
                "{} can be set for new people only: a value set for every row never changes an existing member",
                spec.label
            )));
        }
        if key_fields(import_type).contains(&field) {
            return Err(MappingError(format!(
                "{} can't be set for every row: rows are matched on it",
                spec.label
            )));
        }
        if mapping.contains_key(field) {
            return Err(MappingError(format!(
                "{} comes from a column and is also set for every row; choose one",
                spec.label
            )));
        }
        if field == "kind" && options.default_kind.is_some() {
            return Err(MappingError(
                "Kind is set for every row and also has a default kind; choose one".to_string(),
            ));
        }
        if constant.value.trim().is_empty() {
            return Err(MappingError(format!(
                "the value set for {} is blank",
                spec.label
            )));
        }
        if constant.value.chars().count() > MAX_CELL_CHARS {
            return Err(MappingError(format!(
                "the value set for {} is longer than {MAX_CELL_CHARS} characters",
                spec.label
            )));
        }
    }
    Ok(())
}

/// Check the answers to a file's values: each names a known field, and a
/// replacement or link says what it is.
pub fn validate_answers(answers: &ValueAnswers, fields: &[FieldSpec]) -> Result<(), MappingError> {
    for (field, contexts) in answers {
        let Some(spec) = fields.iter().find(|f| &f.id == field) else {
            return Err(MappingError(format!("unknown field '{field}'")));
        };
        for values in contexts.values() {
            if values.len() > MAX_ROWS {
                return Err(MappingError(format!(
                    "{} has more answers than a file has rows",
                    spec.label
                )));
            }
            for (value, answer) in values {
                if value.chars().count() > MAX_CELL_CHARS {
                    return Err(MappingError(format!(
                        "a value answered for {} is longer than a cell",
                        spec.label
                    )));
                }
                match answer {
                    ValueAnswer::Replace { value: with } if with.trim().is_empty() => {
                        return Err(MappingError(format!(
                            "'{value}' in {} is replaced with a blank; choose Leave blank instead",
                            spec.label
                        )));
                    }
                    ValueAnswer::Match { target } if target.is_null() => {
                        return Err(MappingError(format!(
                            "'{value}' in {} is matched to nothing",
                            spec.label
                        )));
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

/// The grid an importer reads: one column per field, named by its id, in
/// `fields` order, with the mapped column's cells or blanks. Source row
/// numbers carry through.
pub fn apply_mapping(grid: &ParsedCsv, mapping: &ColumnMapping, fields: &[FieldSpec]) -> ParsedCsv {
    let sources: Vec<Option<usize>> = fields.iter().map(|f| mapping.get(&f.id).copied()).collect();
    ParsedCsv {
        headers: fields.iter().map(|f| f.id.clone()).collect(),
        rows: grid
            .rows
            .iter()
            .map(|row| SourceRow {
                source_row: row.source_row,
                cells: sources
                    .iter()
                    .map(|s| {
                        s.and_then(|i| row.cells.get(i))
                            .cloned()
                            .unwrap_or_default()
                    })
                    .collect(),
                cell_errors: Vec::new(),
            })
            .collect(),
        delimiter: grid.delimiter,
    }
}

/// A mapped grid ready for an importer, and the rows held back from it.
pub struct MappedGrid {
    /// The rows the importer checks or writes ([`apply_mapping`]).
    pub grid: ParsedCsv,
    /// Rows with a spreadsheet error in a mapped column, skipped before
    /// the importer sees them so a commit can't write them.
    pub blocked: Vec<RowResult>,
}

impl MappedGrid {
    /// Every data row of the file, held back or not.
    pub fn row_count(&self) -> usize {
        self.grid.rows.len() + self.blocked.len()
    }
}

/// [`apply_mapping`], holding back each row whose mapped cells include a
/// spreadsheet error (`#N/A`). A cell error in a column nobody imports
/// doesn't matter.
pub fn map_grid(grid: &ParsedCsv, mapping: &ColumnMapping, fields: &[FieldSpec]) -> MappedGrid {
    let field_of: HashMap<usize, &str> = mapping
        .iter()
        .map(|(field, &index)| (index, field.as_str()))
        .collect();
    let mut kept = ParsedCsv {
        headers: grid.headers.clone(),
        rows: Vec::with_capacity(grid.rows.len()),
        delimiter: grid.delimiter,
    };
    let mut blocked = Vec::new();
    for row in &grid.rows {
        let errors: Vec<(Option<String>, String)> = row
            .cell_errors
            .iter()
            .filter_map(|&index| {
                let field = field_of.get(&index)?;
                let code = row.cells.get(index).map_or("", String::as_str);
                Some((Some(field.to_string()), cell_error_message(code)))
            })
            .collect();
        if errors.is_empty() {
            kept.rows.push(row.clone());
        } else {
            blocked.push(RowResult::skipped(row.source_row, errors));
        }
    }
    MappedGrid {
        grid: apply_mapping(&kept, mapping, fields),
        blocked,
    }
}

/// [`map_grid`] with the job's constants written into their columns on
/// every row. A constant only new records take is cleared again by the
/// importer on a row that matches an existing record
/// ([`ImportOptions::drop_new_only_constants`]).
pub fn map_job_grid(
    grid: &ParsedCsv,
    mapping: &ColumnMapping,
    options: &ImportOptions,
    fields: &[FieldSpec],
) -> MappedGrid {
    let mut mapped = map_grid(grid, mapping, fields);
    let columns: Vec<(usize, &str)> = options
        .constants
        .iter()
        .filter_map(|c| {
            let index = mapped.grid.headers.iter().position(|h| h == &c.field)?;
            Some((index, c.value.trim()))
        })
        .collect();
    if !columns.is_empty() {
        for row in &mut mapped.grid.rows {
            for &(index, value) in &columns {
                if let Some(cell) = row.cells.get_mut(index) {
                    *cell = value.to_string();
                }
            }
        }
    }
    mapped
}

/// Why a cell holding a spreadsheet error can't import.
pub fn cell_error_message(code: &str) -> String {
    format!("holds the spreadsheet error {code} instead of a value")
}

/// The file's header a field was mapped from.
pub fn source_header(
    mapping: &ColumnMapping,
    headers: &[SourceHeader],
    field: Option<&str>,
) -> Option<String> {
    let index = mapping.get(field?)?;
    headers.get(*index).map(|h| h.name.clone())
}

/// Name each error's field by the header it was mapped from.
pub fn annotate(summary: &mut ImportSummary, mapping: &ColumnMapping, headers: &[SourceHeader]) {
    let source_of = |column: &Option<String>| source_header(mapping, headers, column.as_deref());
    for e in &mut summary.errors {
        e.source_column = source_of(&e.column);
    }
    for g in &mut summary.error_groups {
        g.source_column = source_of(&g.column);
    }
}

/// Finish row results for storing: name each error's field by the header
/// it was mapped from, and keep a skipped row's cells from the file, in
/// source column order, for the error report.
pub fn complete_rows(
    rows: &mut [RowResult],
    mapping: &ColumnMapping,
    headers: &[SourceHeader],
    grid: &ParsedCsv,
) {
    let cells: HashMap<usize, &Vec<String>> =
        grid.rows.iter().map(|r| (r.source_row, &r.cells)).collect();
    for result in rows {
        for e in &mut result.errors {
            e.source_column = source_header(mapping, headers, e.column.as_deref());
        }
        if result.outcome == RowOutcome::Skipped {
            if let Some(row_cells) = cells.get(&result.row) {
                result.cells = (*row_cells).clone();
            }
        }
    }
}

/// The decimal mark a file defaults to: `,` when its fields are separated
/// by `;`, as spreadsheets in comma-decimal locales write them.
pub fn default_decimal(source: &ImportSource) -> DecimalMark {
    match source.delimiter.as_deref() {
        Some(";") => DecimalMark::Comma,
        _ => DecimalMark::Dot,
    }
}

/// Where uploads live: `UPLOAD_DIR/imports`.
pub fn imports_dir() -> PathBuf {
    let root = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "/app/uploads".to_string());
    Path::new(&root).join("imports")
}

/// The normalized grid of a job's file, derived from the job id alone.
pub fn grid_path(job_id: Uuid) -> PathBuf {
    imports_dir().join(format!("{job_id}.grid.json"))
}

pub fn write_grid(job_id: Uuid, grid: &ParsedCsv) -> std::io::Result<()> {
    let path = grid_path(job_id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec(grid).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}

/// A grid written beside the job's current one, put in its place by
/// [`StagedGrid::install`] once the job row agrees, or removed on drop.
pub struct StagedGrid {
    job_id: Uuid,
    path: Option<PathBuf>,
}

impl StagedGrid {
    pub fn write(job_id: Uuid, grid: &ParsedCsv) -> std::io::Result<Self> {
        let dir = imports_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{job_id}.grid.{}.json", Uuid::new_v4()));
        let bytes = serde_json::to_vec(grid).map_err(std::io::Error::other)?;
        std::fs::write(&path, bytes)?;
        Ok(Self {
            job_id,
            path: Some(path),
        })
    }

    /// Replace the job's grid with this one.
    pub fn install(mut self) -> std::io::Result<()> {
        match self.path.take() {
            Some(path) => std::fs::rename(path, grid_path(self.job_id)),
            None => Ok(()),
        }
    }
}

impl Drop for StagedGrid {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Remove every file kept for a job: the upload, its grid and any staged
/// grid. A file already gone is not an error. Returns how many were removed.
pub fn remove_job_files(job_id: Uuid) -> std::io::Result<usize> {
    let entries = match std::fs::read_dir(imports_dir()) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let prefix = format!("{job_id}.");
    let mut removed = 0;
    for entry in entries {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        match std::fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(removed)
}

pub fn read_grid(job_id: Uuid) -> std::io::Result<ParsedCsv> {
    let bytes = std::fs::read(grid_path(job_id))?;
    serde_json::from_slice(&bytes).map_err(std::io::Error::other)
}

/// A JSON object of field ids to column indexes that refuses a field
/// named twice (serde would otherwise keep the last).
pub fn deserialize_mapping<'de, D>(deserializer: D) -> Result<ColumnMapping, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;
    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = ColumnMapping;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an object of field ids to column indexes")
        }
        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut out = ColumnMapping::new();
            while let Some((field, index)) = map.next_entry::<String, usize>()? {
                if out.insert(field.clone(), index).is_some() {
                    return Err(serde::de::Error::custom(format!(
                        "field '{field}' is mapped more than once"
                    )));
                }
            }
            Ok(out)
        }
    }
    deserializer.deserialize_map(Visitor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::imports::types::Constant;
    use crate::services::imports::types::{FieldGroup, ValueType};

    fn field(id: &str, label: &str, required: bool) -> FieldSpec {
        FieldSpec::fixed(
            ImportType::Assets,
            id,
            label,
            FieldGroup::Core,
            ValueType::Text,
            required,
        )
    }

    fn fields() -> Vec<FieldSpec> {
        vec![
            field("name", "Name", true),
            field("kind", "Kind", true),
            field("asset_tag", "Asset tag", false),
            field("serial_number", "Serial number", false),
        ]
    }

    fn headers(names: &[&str]) -> Vec<SourceHeader> {
        names
            .iter()
            .enumerate()
            .map(|(index, name)| SourceHeader {
                index,
                name: name.to_string(),
                samples: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn normalizes_headers() {
        assert_eq!(normalize(" Serial No. "), "serialno");
        assert_eq!(normalize("Asset #"), "assetno");
        assert_eq!(normalize("S/N"), "sn");
        assert_eq!(normalize("asset_tag"), "assettag");
    }

    #[test]
    fn suggests_by_strongest_reason_then_file_order() {
        let synonyms = [("serial", "serial_number"), ("sn", "serial_number")];
        let memory = HeaderMemory {
            choices: HashMap::from([("device".to_string(), Some("name".to_string()))]),
            provenance: None,
        };
        let got = suggest(
            &headers(&[
                "Device",
                "S/N",
                "Serial Number",
                "kind",
                "Row",
                "Errors",
                "Other",
            ]),
            &fields(),
            &synonyms,
            &memory,
        );
        let pairs: Vec<(usize, Option<&str>, SuggestionReason)> = got
            .iter()
            .map(|s| (s.index, s.field.as_deref(), s.reason))
            .collect();
        assert_eq!(
            pairs,
            vec![
                (0, Some("name"), SuggestionReason::Previous),
                // "Serial Number" matches the label, which beats the
                // synonym S/N, so S/N goes unmapped.
                (2, Some("serial_number"), SuggestionReason::Label),
                (3, Some("kind"), SuggestionReason::Exact),
                (4, None, SuggestionReason::Report),
                (5, None, SuggestionReason::Report),
            ]
        );
    }

    #[test]
    fn a_saved_choice_beats_a_guess_and_carries_where_it_came_from() {
        let synonyms = [("serial", "serial_number"), ("comments", "name")];
        let provenance = Provenance {
            job_id: Uuid::nil(),
            by: None,
            by_name: Some("Sam".into()),
            at: chrono::DateTime::UNIX_EPOCH,
        };
        let previous = previous_choices(
            &ImportSource {
                format: "csv".into(),
                sheets: Vec::new(),
                sheet: None,
                header_row: 1,
                headers: headers(&["Comments", "Serial", "Gone", "Left"]),
                row_count: 1,
                delimiter: None,
                preview: Vec::new(),
            },
            &ColumnMapping::from([("serial_number".into(), 1), ("retired".into(), 2)]),
            &[0],
        );
        assert_eq!(previous.get("comments"), Some(&None));
        assert!(!previous.contains_key("left"), "{previous:?}");
        let memory = HeaderMemory {
            choices: previous,
            provenance: Some(provenance.clone()),
        };
        let got = suggest(
            &headers(&["Comments", "Serial", "Gone", "Left", "kind"]),
            &fields(),
            &synonyms,
            &memory,
        );
        let pairs: Vec<(usize, Option<&str>, SuggestionReason, bool)> = got
            .iter()
            .map(|s| {
                (
                    s.index,
                    s.field.as_deref(),
                    s.reason,
                    s.provenance.as_ref() == Some(&provenance),
                )
            })
            .collect();
        assert_eq!(
            pairs,
            vec![
                // Saved as Don't import: the synonym to name doesn't apply.
                (0, None, SuggestionReason::Previous, true),
                (1, Some("serial_number"), SuggestionReason::Previous, true),
                // "Gone" was mapped to a field that no longer exists, and
                // "Left" had no choice: neither is remembered.
                (4, Some("kind"), SuggestionReason::Exact, false),
            ]
        );
    }

    #[test]
    fn ignored_columns_are_checked() {
        let h = headers(&["Name", "Kind", "Notes"]);
        let mapping = ColumnMapping::from([("name".into(), 0)]);
        assert!(validate_ignored(&[2, 1], &mapping, &h).is_ok());
        let refused = [
            (vec![3], "file has 3 columns"),
            (vec![2, 2], "twice"),
            (vec![0], "also set to Don't import"),
        ];
        for (ignored, expected) in refused {
            let err = validate_ignored(&ignored, &mapping, &h).unwrap_err().0;
            assert!(err.contains(expected), "{expected}: {err}");
        }
    }

    #[test]
    fn validation_refusals() {
        let h = headers(&["Name", "Kind", "Tag"]);
        let opts = ImportOptions::default();
        let ok = ColumnMapping::from([("name".into(), 0), ("kind".into(), 1)]);
        assert!(validate(ImportType::Assets, &ok, &opts, &fields(), &h).is_ok());

        let unknown =
            ColumnMapping::from([("name".into(), 0), ("kind".into(), 1), ("x".into(), 2)]);
        assert!(validate(ImportType::Assets, &unknown, &opts, &fields(), &h)
            .unwrap_err()
            .0
            .contains("unknown field 'x'"));

        let twice = ColumnMapping::from([
            ("name".into(), 0),
            ("kind".into(), 1),
            ("asset_tag".into(), 0),
        ]);
        let err = validate(ImportType::Assets, &twice, &opts, &fields(), &h).unwrap_err();
        assert!(err.0.contains("column 'Name' is mapped to both"), "{err:?}");

        let out_of_range = ColumnMapping::from([("name".into(), 0), ("kind".into(), 7)]);
        assert!(validate(ImportType::Assets, &out_of_range, &opts, &fields(), &h).is_err());

        let no_kind = ColumnMapping::from([("name".into(), 0)]);
        let err = validate(ImportType::Assets, &no_kind, &opts, &fields(), &h).unwrap_err();
        assert!(err.0.contains("Kind"), "{err:?}");
        let defaulted = ImportOptions {
            default_kind: Some("generic".into()),
            ..Default::default()
        };
        assert!(validate(ImportType::Assets, &no_kind, &defaulted, &fields(), &h).is_ok());

        let no_name = ColumnMapping::from([("kind".into(), 1)]);
        assert_eq!(
            validate(ImportType::Assets, &no_name, &opts, &fields(), &h).unwrap_err(),
            MappingError("no column maps to Name".into())
        );
    }

    fn constant(field: &str, value: &str, apply_to: ApplyTo) -> Constant {
        Constant {
            field: field.to_string(),
            value: value.to_string(),
            apply_to,
        }
    }

    #[test]
    fn constants_cover_fields_and_are_refused_where_they_cannot_apply() {
        let h = headers(&["Name", "Tag"]);
        let mapping = ColumnMapping::from([("name".into(), 0), ("asset_tag".into(), 1)]);
        let with = |constants: Vec<Constant>| ImportOptions {
            constants,
            ..Default::default()
        };
        let check = |options: &ImportOptions| {
            validate(ImportType::Assets, &mapping, options, &fields(), &h).map_err(|e| e.0)
        };

        // A constant covers a required field no column feeds.
        assert!(check(&with(vec![constant("kind", "camera", ApplyTo::New)])).is_ok());

        let refused = [
            (vec![constant("x", "1", ApplyTo::New)], "unknown field 'x'"),
            (
                vec![
                    constant("name", "Cam", ApplyTo::New),
                    constant("kind", "c", ApplyTo::New),
                ],
                "Name comes from a column",
            ),
            (
                vec![
                    constant("kind", "a", ApplyTo::New),
                    constant("kind", "b", ApplyTo::All),
                ],
                "Kind is set for every row twice",
            ),
            (
                vec![
                    constant("kind", "c", ApplyTo::New),
                    constant("serial_number", "S", ApplyTo::New),
                ],
                "rows are matched on it",
            ),
            (vec![constant("kind", " ", ApplyTo::New)], "is blank"),
            (
                vec![constant(
                    "kind",
                    &"x".repeat(MAX_CELL_CHARS + 1),
                    ApplyTo::New,
                )],
                "longer than",
            ),
        ];
        for (constants, expected) in refused {
            let err = check(&with(constants)).unwrap_err();
            assert!(err.contains(expected), "{expected}: {err}");
        }

        let both = ImportOptions {
            default_kind: Some("generic".into()),
            constants: vec![constant("kind", "camera", ApplyTo::New)],
            ..Default::default()
        };
        assert!(check(&both).unwrap_err().contains("default kind"));

        // A constant never changes an existing member.
        let user_fields = vec![field("email", "Email", true), field("role", "Role", true)];
        let user_mapping = ColumnMapping::from([("email".into(), 0)]);
        let role = |apply_to| ImportOptions {
            constants: vec![constant("role", "user", apply_to)],
            ..Default::default()
        };
        assert!(validate(
            ImportType::Users,
            &user_mapping,
            &role(ApplyTo::New),
            &user_fields,
            &h
        )
        .is_ok());
        assert!(validate(
            ImportType::Users,
            &user_mapping,
            &role(ApplyTo::All),
            &user_fields,
            &h
        )
        .unwrap_err()
        .0
        .contains("new people only"));
    }

    #[test]
    fn constants_fill_their_column_on_every_row() {
        let grid = ParsedCsv {
            headers: vec!["Name".into()],
            rows: vec![
                SourceRow {
                    source_row: 2,
                    cells: vec!["A".into()],
                    cell_errors: Vec::new(),
                },
                SourceRow {
                    source_row: 3,
                    cells: vec!["B".into()],
                    cell_errors: Vec::new(),
                },
            ],
            delimiter: b',',
        };
        let mapping = ColumnMapping::from([("name".into(), 0)]);
        let options = ImportOptions {
            constants: vec![constant("kind", " camera ", ApplyTo::New)],
            ..Default::default()
        };
        let mapped = map_job_grid(&grid, &mapping, &options, &fields());
        let kinds: Vec<String> = mapped
            .grid
            .records()
            .map(|r| r.values["kind"].clone())
            .collect();
        assert_eq!(kinds, vec!["camera", "camera"]);

        let mut row = mapped.grid.records().next().unwrap().values;
        options.drop_new_only_constants(&mut row);
        assert_eq!(row["kind"], "");
        assert_eq!(row["name"], "A");
    }

    #[test]
    fn options_and_answers_keep_their_shape() {
        let options: ImportOptions = serde_json::from_value(serde_json::json!({
            "constants": [
                { "field": "location", "value": "Studio B" },
                { "field": "kind", "value": "camera", "apply_to": "all" },
            ]
        }))
        .unwrap();
        assert_eq!(options.constants[0].apply_to, ApplyTo::New);
        assert_eq!(options.constants[1].apply_to, ApplyTo::All);
        assert!(serde_json::from_value::<ImportOptions>(serde_json::json!({
            "constants": [{ "field": "kind", "value": "c", "apply_to": "some" }]
        }))
        .is_err());

        let answers: ValueAnswers = serde_json::from_value(serde_json::json!({
            "kind": { "": {
                "cam": { "action": "match", "target": "camera" },
                "spare": { "action": "skip" },
            }},
            "serial_number": { "": { "n/a": { "action": "leave_blank" } } },
            "name": { "": { "Cam1": { "action": "replace", "value": "Cam 1" } } },
        }))
        .unwrap();
        assert_eq!(
            answers["kind"][""]["cam"],
            ValueAnswer::Match {
                target: serde_json::json!("camera")
            }
        );
        assert!(validate_answers(&answers, &fields()).is_ok());
        let back = serde_json::to_value(&answers).unwrap();
        assert_eq!(
            back["kind"][""]["spare"],
            serde_json::json!({ "action": "skip" })
        );

        let refused = [
            (
                serde_json::json!({ "x": { "": { "a": { "action": "skip" } } } }),
                "unknown field",
            ),
            (
                serde_json::json!({ "name": { "": { "a": { "action": "replace", "value": " " } } } }),
                "replaced with a blank",
            ),
            (
                serde_json::json!({ "kind": { "": { "a": { "action": "match", "target": null } } } }),
                "matched to nothing",
            ),
        ];
        for (raw, expected) in refused {
            let answers: ValueAnswers = serde_json::from_value(raw).unwrap();
            let err = validate_answers(&answers, &fields()).unwrap_err().0;
            assert!(err.contains(expected), "{expected}: {err}");
        }
        assert!(serde_json::from_value::<ValueAnswers>(serde_json::json!({
            "kind": { "": { "a": { "action": "guess" } } }
        }))
        .is_err());
    }

    #[test]
    fn mapping_refuses_a_field_named_twice() {
        #[derive(Deserialize)]
        struct Body {
            #[serde(deserialize_with = "deserialize_mapping")]
            mapping: ColumnMapping,
        }
        let ok: Body = serde_json::from_str(r#"{"mapping":{"name":0,"kind":1}}"#).unwrap();
        assert_eq!(ok.mapping.len(), 2);
        let err = serde_json::from_str::<Body>(r#"{"mapping":{"name":0,"name":1}}"#)
            .err()
            .unwrap();
        assert!(err.to_string().contains("mapped more than once"), "{err}");
    }

    #[test]
    fn apply_mapping_emits_every_field() {
        let grid = ParsedCsv {
            headers: vec!["Tag".into(), "Title".into(), "Ignored".into()],
            rows: vec![SourceRow {
                source_row: 4,
                cells: vec!["T-1".into(), "Camera".into(), "x".into()],
                cell_errors: Vec::new(),
            }],
            delimiter: b',',
        };
        let mapping = ColumnMapping::from([("asset_tag".into(), 0), ("name".into(), 1)]);
        let mapped = apply_mapping(&grid, &mapping, &fields());
        assert_eq!(
            mapped.headers,
            vec!["name", "kind", "asset_tag", "serial_number"]
        );
        assert_eq!(mapped.rows[0].source_row, 4);
        assert_eq!(mapped.rows[0].cells, vec!["Camera", "", "T-1", ""]);
    }

    #[test]
    fn map_grid_holds_back_rows_with_a_mapped_cell_error() {
        let grid = ParsedCsv {
            headers: vec!["Tag".into(), "Title".into(), "Lookup".into()],
            rows: vec![
                SourceRow {
                    source_row: 4,
                    cells: vec!["T-1".into(), "Camera".into(), "#REF!".into()],
                    cell_errors: vec![2],
                },
                SourceRow {
                    source_row: 6,
                    cells: vec!["T-2".into(), "#N/A".into(), "".into()],
                    cell_errors: vec![1],
                },
            ],
            delimiter: b',',
        };
        let mapping = ColumnMapping::from([("asset_tag".into(), 0), ("name".into(), 1)]);
        let mapped = map_grid(&grid, &mapping, &fields());
        assert_eq!(mapped.row_count(), 2);
        // The error in the unmapped Lookup column doesn't hold row 4 back.
        assert_eq!(mapped.grid.rows.len(), 1);
        assert_eq!(mapped.grid.rows[0].source_row, 4);
        assert_eq!(mapped.blocked.len(), 1);
        let blocked = &mapped.blocked[0];
        assert_eq!((blocked.row, blocked.outcome), (6, RowOutcome::Skipped));
        assert_eq!(blocked.errors[0].column.as_deref(), Some("name"));
        assert!(blocked.errors[0].message.contains("#N/A"), "{blocked:?}");
    }
}
