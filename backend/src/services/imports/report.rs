//! The error report: every skipped row of an import with why it was
//! skipped, and every row that imports without some of its values with
//! which ones, in the file's own layout and order.
//!
//! Columns are `Row` (the row's line in the file), `Errors` ("Header:
//! message; ..."), then the file's columns in source order. The report is
//! written with the file's own delimiter, so a `;` file comes back with
//! its `,` decimals readable, and every cell goes through the formula
//! guard, which the parser strips again on upload. Fix the rows, upload
//! the report, and `Row` and `Errors` map to nothing.

use super::mapping::{is_report_header, SourceHeader};
use super::safe_cells::csv_formula_safe;
use super::types::{CellError, DroppedCell, FieldSpec, RowOutcome, RowPhase, RowResult};

/// The report's own leading columns.
pub const REPORT_COLUMNS: [&str; 2] = ["Row", "Errors"];

/// Build the report from a job's row results, in file order: each skipped
/// row with its errors, and each row that imports without some of its
/// values (`RowResult::dropped`) with which ones, led by whether it was
/// imported or will be. `headers` are the file's headers; a column that is
/// itself an earlier report's `Row` or `Errors` is left out, so a report of
/// a report has each column once. `fields` name errors on fields no column
/// maps to. A skipped row keeps its cells; `cells_of` gives an imported
/// row's from the file.
pub fn error_report_csv<'a>(
    headers: &[SourceHeader],
    delimiter: u8,
    rows: &'a [RowResult],
    fields: &[FieldSpec],
    phase: RowPhase,
    kind_label: impl Fn(&str) -> String,
    cells_of: impl Fn(usize) -> &'a [String],
) -> Result<Vec<u8>, csv::Error> {
    let lines = rows.iter().filter_map(|r| {
        if r.outcome == RowOutcome::Skipped {
            return Some(ReportLine {
                row: r.row,
                errors: errors_text(&r.errors, fields),
                cells: &r.cells,
            });
        }
        if r.dropped.is_empty() {
            return None;
        }
        let dropped: Vec<&DroppedCell> = r.dropped.iter().collect();
        Some(ReportLine {
            row: r.row,
            errors: partial_text(phase, &dropped, fields, &kind_label),
            cells: cells_of(r.row),
        })
    });
    report_csv(headers, delimiter, lines)
}

/// Whether any row imports without some of its values, so the report
/// needs the file's cells for it.
pub fn has_partial_rows(rows: &[RowResult]) -> bool {
    rows.iter()
        .any(|r| r.outcome != RowOutcome::Skipped && !r.dropped.is_empty())
}

/// A partly imported row's `Errors` cell: a leading clause that says it
/// was imported (or will be, before the commit), then [`dropped_text`].
pub fn partial_text(
    phase: RowPhase,
    cells: &[&DroppedCell],
    fields: &[FieldSpec],
    kind_label: impl Fn(&str) -> String,
) -> String {
    let lead = match phase {
        RowPhase::Checked => "Will be imported without these values",
        RowPhase::Committed => "Imported without these values",
    };
    format!("{lead}; {}", dropped_text(cells, fields, kind_label))
}

/// One row of a report: its line in the file, why it is listed, and its
/// cells in source column order.
pub struct ReportLine<'a> {
    pub row: usize,
    pub errors: String,
    pub cells: &'a [String],
}

/// A report of `lines` in the file's layout: `Row`, `Errors`, then the
/// file's columns less an earlier report's own.
pub fn report_csv<'a>(
    headers: &[SourceHeader],
    delimiter: u8,
    lines: impl Iterator<Item = ReportLine<'a>>,
) -> Result<Vec<u8>, csv::Error> {
    let kept: Vec<&SourceHeader> = headers
        .iter()
        .filter(|h| !is_report_header(&h.name))
        .collect();

    // UTF-8 BOM so Excel reads the file as UTF-8.
    let mut buf = "\u{FEFF}".as_bytes().to_vec();
    {
        let mut wtr = csv::WriterBuilder::new()
            .delimiter(delimiter)
            .from_writer(&mut buf);
        let header_row = REPORT_COLUMNS
            .iter()
            .map(|c| c.to_string())
            .chain(kept.iter().map(|h| h.name.clone()))
            .map(csv_formula_safe);
        wtr.write_record(header_row)?;
        for line in lines {
            let cells = kept
                .iter()
                .map(|h| line.cells.get(h.index).cloned().unwrap_or_default());
            let record = [line.row.to_string(), line.errors]
                .into_iter()
                .chain(cells)
                .map(csv_formula_safe);
            wtr.write_record(record)?;
        }
        wtr.flush()?;
    }
    Ok(buf)
}

/// Why a row imports without some of its values, as one cell: "Header:
/// reason; Header: reason". `kind_label` names a kind by its slug.
pub fn dropped_text(
    cells: &[&DroppedCell],
    fields: &[FieldSpec],
    kind_label: impl Fn(&str) -> String,
) -> String {
    cells
        .iter()
        .map(|d| {
            let name = d.source_column.clone().unwrap_or_else(|| {
                fields
                    .iter()
                    .find(|f| f.id == d.column)
                    .map_or_else(|| d.column.clone(), |f| f.label.clone())
            });
            let reason = match (d.column.as_str(), &d.value) {
                ("primary_user", Some(value)) => {
                    format!("'{value}' is not a member of this workspace, left unassigned")
                }
                (_, Some(value)) => format!("'{value}' left off"),
                (_, None) => format!("{} assets have no {name}, left out", kind_label(&d.kind)),
            };
            format!("{name}: {reason}")
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// A row's errors as one cell: "Header: message; Header: message". The
/// header is the file's own, or the field's label when no column maps to
/// it; a whole-row error is its message alone.
pub fn errors_text(errors: &[CellError], fields: &[FieldSpec]) -> String {
    errors
        .iter()
        .map(|e| {
            let name = e.source_column.clone().or_else(|| {
                e.column.as_ref().map(|id| {
                    fields
                        .iter()
                        .find(|f| &f.id == id)
                        .map_or_else(|| id.clone(), |f| f.label.clone())
                })
            });
            match name {
                Some(name) => format!("{name}: {}", e.message),
                None => e.message.clone(),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::imports::csv_parser;
    use crate::services::imports::types::{FieldGroup, ImportType, ValueType};

    fn header(index: usize, name: &str) -> SourceHeader {
        SourceHeader {
            index,
            name: name.to_string(),
            samples: Vec::new(),
        }
    }

    fn skipped(row: usize, cells: &[&str], errors: Vec<(Option<&str>, &str)>) -> RowResult {
        let mut r = RowResult::skipped(
            row,
            errors
                .into_iter()
                .map(|(c, m)| (c.map(String::from), m.to_string()))
                .collect(),
        );
        r.cells = cells.iter().map(|c| c.to_string()).collect();
        r
    }

    #[test]
    fn report_lists_skipped_rows_in_the_file_layout() {
        let headers = vec![header(0, "Asset #"), header(1, "Title"), header(2, "Notes")];
        let mut bad = skipped(
            7,
            &["TVS-1", "=SUM(A1)", "-tripod"],
            vec![
                (Some("asset_tag"), "tag 'TVS-1' appears twice"),
                (None, "whole row"),
            ],
        );
        bad.errors[0].source_column = Some("Asset #".into());
        let fields = vec![FieldSpec::fixed(
            ImportType::Assets,
            "kind",
            "Kind",
            FieldGroup::Core,
            ValueType::Enum,
            true,
        )];
        let kind_missing = skipped(
            9,
            &["T-2", "Cam", ""],
            vec![(Some("kind"), "kind is required")],
        );
        let fine = RowResult::planned(8, RowOutcome::Create);

        let bytes = error_report_csv(
            &headers,
            b',',
            &[bad, fine, kind_missing],
            &fields,
            RowPhase::Checked,
            |s| s.to_string(),
            |_| &[],
        )
        .unwrap();
        assert!(bytes.starts_with("\u{FEFF}".as_bytes()));
        let text = String::from_utf8(bytes).unwrap();
        let lines: Vec<&str> = text.trim_start_matches('\u{FEFF}').lines().collect();
        assert_eq!(lines[0], "Row,Errors,Asset #,Title,Notes");
        assert_eq!(
            lines[1],
            "7,Asset #: tag 'TVS-1' appears twice; whole row,TVS-1,'=SUM(A1),'-tripod"
        );
        assert_eq!(lines[2], "9,Kind: kind is required,T-2,Cam,");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn report_of_a_report_keeps_one_row_and_errors_column() {
        let headers = vec![header(0, "Row"), header(1, "Errors"), header(2, "name")];
        let row = skipped(2, &["5", "old", "Camera"], vec![(None, "new problem")]);
        let bytes = error_report_csv(
            &headers,
            b';',
            &[row],
            &[],
            RowPhase::Checked,
            |s| s.to_string(),
            |_| &[],
        )
        .unwrap();
        let parsed = csv_parser::parse_bytes(&bytes).unwrap();
        assert_eq!(parsed.headers, vec!["Row", "Errors", "name"]);
        assert_eq!(parsed.delimiter, b';');
        assert_eq!(parsed.rows[0].cells, vec!["2", "new problem", "Camera"]);
    }

    fn partly(row: usize, outcome: RowOutcome, dropped: Vec<(&str, Option<&str>)>) -> RowResult {
        let mut r = RowResult::planned(row, outcome);
        r.dropped = dropped
            .into_iter()
            .map(|(column, value)| DroppedCell {
                column: column.to_string(),
                source_column: match column {
                    "primary_user" => Some("Assigned To".to_string()),
                    _ => None,
                },
                kind: "camera".to_string(),
                value: value.map(String::from),
            })
            .collect();
        r
    }

    #[test]
    fn report_lists_partly_imported_rows_among_skipped_ones_in_file_order() {
        let headers = vec![
            header(0, "Tag"),
            header(1, "Assigned To"),
            header(2, "Rego"),
        ];
        let fields = vec![FieldSpec::fixed(
            ImportType::Assets,
            "rego",
            "Rego",
            FieldGroup::Core,
            ValueType::Text,
            false,
        )];
        let rows = vec![
            partly(
                2,
                RowOutcome::Create,
                vec![("primary_user", Some("nobody@example.com")), ("rego", None)],
            ),
            skipped(3, &["T-3", "", ""], vec![(None, "whole row")]),
            RowResult::planned(4, RowOutcome::Update),
            partly(
                5,
                RowOutcome::Unchanged,
                vec![("groups", Some("Field kit"))],
            ),
        ];
        let file: std::collections::HashMap<usize, Vec<String>> = [
            (
                2,
                vec!["T-2".into(), "nobody@example.com".into(), "XYZ".into()],
            ),
            (5, vec!["T-5".into(), String::new(), String::new()]),
        ]
        .into_iter()
        .collect();
        let report = |phase| {
            let bytes = error_report_csv(
                &headers,
                b',',
                &rows,
                &fields,
                phase,
                |_| "Camera".to_string(),
                |row| file.get(&row).map_or(&[][..], |c| c.as_slice()),
            )
            .unwrap();
            String::from_utf8(bytes).unwrap()
        };

        let checked = report(RowPhase::Checked);
        let lines: Vec<&str> = checked.trim_start_matches('\u{FEFF}').lines().collect();
        assert_eq!(
            lines,
            [
                "Row,Errors,Tag,Assigned To,Rego",
                "2,\"Will be imported without these values; Assigned To: 'nobody@example.com' is not a member of this workspace, left unassigned; Rego: Camera assets have no Rego, left out\",T-2,nobody@example.com,XYZ",
                "3,whole row,T-3,,",
                "5,Will be imported without these values; groups: 'Field kit' left off,T-5,,",
            ]
        );

        let committed = report(RowPhase::Committed);
        let lines: Vec<&str> = committed.trim_start_matches('\u{FEFF}').lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(
            lines[1].starts_with("2,\"Imported without these values; Assigned To: "),
            "{}",
            lines[1]
        );
        assert_eq!(
            lines[3],
            "5,Imported without these values; groups: 'Field kit' left off,T-5,,"
        );
        assert!(has_partial_rows(&rows));
        assert!(!has_partial_rows(&rows[1..3]));
    }
}
