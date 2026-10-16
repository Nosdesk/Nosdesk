//! Download previous values (design r2 §9): the values an asset import's
//! updates replaced, as a file the importer reads back. One row per
//! updated asset, keyed by the field the import matched on, then every
//! field the import wrote, each holding the value before it. Name and
//! kind are there only when the import wrote them: a row that matches an
//! asset leaves out what the file doesn't give.
//!
//! A field that was blank before has an empty cell, and an empty cell
//! never changes a value, so importing the file can't blank it again;
//! [`PreviousValues::blank_before`] counts those. Groups are left out:
//! an import only adds to an asset's groups, so a file can't take the
//! added ones away.

use std::collections::BTreeSet;

use serde::Serialize;

use super::assets::TEMPLATE_CORE_HEADERS;
use super::safe_cells::csv_formula_safe;
use super::{MatchBy, RowOutcome, RowResult};

/// Written by an update but not offered back: an import adds groups and
/// never removes them.
const NOT_RESTORABLE: &[&str] = &["groups"];

/// The file's size, for the detail page's help text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PreviousValues {
    /// Updated assets in the file.
    pub rows: usize,
    /// Cells for a field that was blank before the import wrote it.
    pub blank_before: usize,
}

/// The column the file matches on: the job's own key.
pub fn key_field(key: MatchBy) -> &'static str {
    match key {
        MatchBy::SerialNumber => "serial_number",
        MatchBy::AssetTag | MatchBy::None => "asset_tag",
    }
}

/// The fields the file holds, in the template's order with attributes
/// after the core fields.
fn columns(rows: &[&RowResult], key: &str) -> Vec<String> {
    let written: BTreeSet<&str> = rows
        .iter()
        .flat_map(|r| r.fields_written.keys())
        .map(String::as_str)
        .filter(|f| *f != key && !NOT_RESTORABLE.contains(f))
        .collect();
    let rank = |f: &str| {
        TEMPLATE_CORE_HEADERS
            .iter()
            .position(|h| *h == f)
            .unwrap_or(TEMPLATE_CORE_HEADERS.len())
    };
    let mut out: Vec<String> = written.into_iter().map(str::to_string).collect();
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
    out
}

/// The updated rows with something to put back and a key to match on.
fn updated<'a>(rows: &'a [RowResult], key: &str) -> Vec<&'a RowResult> {
    rows.iter()
        .filter(|r| r.outcome == RowOutcome::Update)
        .filter(|r| r.key.as_deref().is_some_and(|k| !k.trim().is_empty()))
        .filter(|r| {
            r.fields_written
                .keys()
                .any(|f| f.as_str() != key && !NOT_RESTORABLE.contains(&f.as_str()))
        })
        .collect()
}

/// A row's cell for `field`: the value before the import, blank for a
/// field the row didn't write.
fn before(row: &RowResult, field: &str) -> Option<String> {
    row.fields_written
        .get(field)
        .and_then(|change| change.before.clone())
}

/// How many assets the file has and how many of their fields were blank.
pub fn counts(rows: &[RowResult], key: MatchBy) -> PreviousValues {
    let key = key_field(key);
    let rows = updated(rows, key);
    let columns = columns(&rows, key);
    let blank_before = rows
        .iter()
        .map(|r| {
            columns
                .iter()
                .filter_map(|c| r.fields_written.get(c))
                .filter(|change| change.before.as_deref().is_none_or(|v| v.trim().is_empty()))
                .count()
        })
        .sum();
    PreviousValues {
        rows: rows.len(),
        blank_before,
    }
}

/// The file: a UTF-8 BOM so Excel reads it as UTF-8, the key column, then
/// each field's value before the import; a field the row didn't write is
/// blank and so left as it is.
pub fn csv(rows: &[RowResult], key: MatchBy) -> Result<Vec<u8>, csv::Error> {
    let key = key_field(key);
    let rows = updated(rows, key);
    let columns = columns(&rows, key);
    let mut buf = "\u{FEFF}".as_bytes().to_vec();
    {
        let mut wtr = csv::Writer::from_writer(&mut buf);
        wtr.write_record(
            std::iter::once(key.to_string())
                .chain(columns.iter().cloned())
                .map(csv_formula_safe),
        )?;
        for row in rows {
            let cells = columns.iter().map(|c| before(row, c).unwrap_or_default());
            wtr.write_record(
                std::iter::once(row.key.clone().unwrap_or_default())
                    .chain(cells)
                    .map(csv_formula_safe),
            )?;
        }
        wtr.flush()?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::imports::{FieldChange, RecordId};

    fn update(row: usize, tag: &str, changes: &[(&str, Option<&str>, Option<&str>)]) -> RowResult {
        let mut r = RowResult::planned(row, RowOutcome::Update)
            .with_record(RecordId::Int(row as i32))
            .with_key(Some(tag.to_string()))
            .with_label(format!("Cam {row}"));
        r.kind = Some("camera".to_string());
        for (field, before, after) in changes {
            r.fields_written.insert(
                field.to_string(),
                FieldChange {
                    before: before.map(str::to_string),
                    after: after.map(str::to_string),
                },
            );
        }
        r
    }

    fn lines(bytes: &[u8]) -> Vec<String> {
        String::from_utf8(bytes.to_vec())
            .unwrap()
            .trim_start_matches('\u{FEFF}')
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn one_row_per_update_with_the_values_before() {
        let rows = vec![
            update(
                2,
                "T-1",
                &[("location", Some("Studio A"), Some("Field kit 2"))],
            ),
            update(
                3,
                "T-2",
                &[
                    ("notes", None, Some("new")),
                    ("status", Some("in_repair"), Some("in_service")),
                    ("groups", Some("A"), Some("A, B")),
                ],
            ),
            RowResult::planned(4, RowOutcome::Create).with_key(Some("T-3".to_string())),
            update(5, "T-4", &[("groups", None, Some("B"))]),
        ];
        let file = csv(&rows, MatchBy::AssetTag).unwrap();
        assert_eq!(
            lines(&file),
            vec![
                "asset_tag,status,location,notes".to_string(),
                "T-1,,Studio A,".to_string(),
                "T-2,in_repair,,".to_string(),
            ]
        );
        assert_eq!(
            counts(&rows, MatchBy::AssetTag),
            PreviousValues {
                rows: 2,
                blank_before: 1
            }
        );
    }

    #[test]
    fn a_serial_matched_import_keys_the_file_by_serial() {
        let rows = vec![update(
            2,
            "SN-1",
            &[
                ("asset_tag", Some("OLD-1"), Some("NEW-1")),
                ("name", Some("Old name"), Some("New name")),
                ("attr:mount", Some("=EF"), Some("PL")),
            ],
        )];
        let file = csv(&rows, MatchBy::SerialNumber).unwrap();
        assert_eq!(
            lines(&file),
            vec![
                "serial_number,name,asset_tag,attr:mount".to_string(),
                "SN-1,Old name,OLD-1,'=EF".to_string(),
            ]
        );
    }

    #[test]
    fn nothing_updated_is_a_header_only_file() {
        let rows = vec![RowResult::planned(2, RowOutcome::Unchanged)];
        assert_eq!(
            lines(&csv(&rows, MatchBy::AssetTag).unwrap()),
            vec!["asset_tag"]
        );
        assert_eq!(counts(&rows, MatchBy::AssetTag), PreviousValues::default());
    }
}
