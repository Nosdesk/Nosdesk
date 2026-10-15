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

use super::csv_parser::{ParsedCsv, SourceRow};
use super::types::{DecimalMark, FieldSpec, ImportOptions, ImportSummary, ImportType};

/// Sample values carried per header.
pub const SAMPLES: usize = 3;
/// Longest sample value, in characters.
const SAMPLE_CHARS: usize = 100;

/// Headers of the error report's own columns. A re-uploaded report maps
/// them to nothing.
const REPORT_HEADERS: &[&str] = &["row", "errors"];

/// Field id to the 0-based index of the column that feeds it.
pub type ColumnMapping = BTreeMap<String, usize>;

/// The uploaded file as the Map step shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSource {
    /// `csv` (xlsx arrives in a later slice).
    pub format: String,
    /// Workbook sheets; empty for CSV.
    #[serde(default)]
    pub sheets: Vec<SheetInfo>,
    pub sheet: Option<String>,
    /// 1-based line of the header row.
    pub header_row: usize,
    pub headers: Vec<SourceHeader>,
    pub row_count: usize,
    /// The CSV field separator, so the client can default the decimal mark.
    #[serde(default)]
    pub delimiter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SheetInfo {
    pub name: String,
    pub visible: bool,
    pub rows: usize,
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
    /// The same header mapped this way in the type's last mapped import.
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
}

/// What the `source` column stores: the source plus the suggestions made
/// at upload, so a resumed job shows the same suggestions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSource {
    #[serde(flatten)]
    pub source: ImportSource,
    #[serde(default)]
    pub suggested_mapping: Vec<Suggestion>,
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

/// Describe a parsed CSV for the Map step.
pub fn csv_source(parsed: &ParsedCsv) -> ImportSource {
    let headers = parsed
        .headers
        .iter()
        .enumerate()
        .map(|(index, name)| SourceHeader {
            index,
            name: name.clone(),
            samples: parsed
                .rows
                .iter()
                .map(|r| r.cells[index].as_str())
                .filter(|v| !v.is_empty())
                .take(SAMPLES)
                .map(|v| v.chars().take(SAMPLE_CHARS).collect())
                .collect(),
        })
        .collect();
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
    }
}

/// The mapping a previous job of the same type used, as normalized header
/// to field id.
pub fn previous_choices(source: &ImportSource, mapping: &ColumnMapping) -> HashMap<String, String> {
    mapping
        .iter()
        .filter_map(|(field, &index)| {
            let header = source.headers.get(index)?;
            Some((normalize(&header.name), field.clone()))
        })
        .collect()
}

/// Pre-map each header. Each field goes to one column at most: the
/// strongest reason wins (previous, exact, label, synonym), then file
/// order. A blank header gets no suggestion.
pub fn suggest(
    headers: &[SourceHeader],
    fields: &[FieldSpec],
    synonyms: &[(&str, &str)],
    previous: &HashMap<String, String>,
) -> Vec<Suggestion> {
    let known: HashSet<&str> = fields.iter().map(|f| f.id.as_str()).collect();
    let mut candidates: Vec<(SuggestionReason, usize, String)> = Vec::new();
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
        if let Some(field) = previous.get(&norm).filter(|f| known.contains(f.as_str())) {
            candidates.push((SuggestionReason::Previous, header.index, field.clone()));
        }
        if let Some(field) = fields.iter().find(|f| f.id == raw) {
            candidates.push((SuggestionReason::Exact, header.index, field.id.clone()));
        }
        if let Some(field) = fields
            .iter()
            .find(|f| normalize(&f.label) == norm || normalize(&f.id) == norm)
        {
            candidates.push((SuggestionReason::Label, header.index, field.id.clone()));
        }
        if let Some((_, field)) = synonyms
            .iter()
            .find(|(s, f)| *s == norm && known.contains(f))
        {
            candidates.push((SuggestionReason::Synonym, header.index, field.to_string()));
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
        })
        .collect();
    for (reason, index, field) in candidates {
        if taken_columns.contains(&index) || taken_fields.contains(&field) {
            continue;
        }
        taken_columns.insert(index);
        taken_fields.insert(field.clone());
        out.push(Suggestion {
            index,
            field: Some(field),
            reason,
        });
    }
    out.sort_by_key(|s| s.index);
    out
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
    for spec in fields.iter().filter(|f| f.required) {
        if mapping.contains_key(&spec.id) {
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
            })
            .collect(),
        delimiter: grid.delimiter,
    }
}

/// Name each error's field by the header it was mapped from.
pub fn annotate(summary: &mut ImportSummary, mapping: &ColumnMapping, headers: &[SourceHeader]) {
    let source_of = |column: &Option<String>| {
        let index = mapping.get(column.as_deref()?)?;
        headers.get(*index).map(|h| h.name.clone())
    };
    for e in &mut summary.errors {
        e.source_column = source_of(&e.column);
    }
    for g in &mut summary.error_groups {
        g.source_column = source_of(&g.column);
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
        let previous = HashMap::from([("device".to_string(), "name".to_string())]);
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
            &previous,
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
}
