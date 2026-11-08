//! The import template as an .xlsx workbook, built from the type's fields
//! so every import type gets it from the same code.
//!
//! - **Template**: the header row of field ids, as the CSV template has it,
//!   bold and frozen. Rows 2 to 5,001 (the row cap) carry Warning-style
//!   data validation: a dropdown for each closed set (the labels Match
//!   values reads as Exact) and for yes or no, pointing at a range on the
//!   Lists sheet; dates between 1900 and 9999 shown as `YYYY-MM-DD`;
//!   numbers in the range the importer stores. A value outside a list can
//!   still go in, and Match values asks about it.
//! - **Fields**: one row per column: id, label, type, whether it's
//!   required (or for which kinds), allowed values and format.
//! - **Lists**: hidden; one column per dropdown, its field id on row 1.
//!
//! Every cell is written as a string, so a workspace-named value such as a
//! kind called `=1+1` stays text. The only formulas are the list ranges.

use rust_xlsxwriter::{
    DataValidation, DataValidationErrorStyle, DataValidationRule, ExcelDateTime, Format, Formula,
    Workbook, Worksheet, XlsxError,
};

use crate::db::DbConnection;

use super::csv_parser::{MAX_CELL_CHARS, MAX_ROWS};
use super::types::{FieldSpec, Importer, ValueType};

const TEMPLATE_SHEET: &str = "Template";
const FIELDS_SHEET: &str = "Fields";
const LISTS_SHEET: &str = "Lists";

/// Yes and no as a boolean field's choices label them
/// (`values::boolean_matcher`).
const YES_NO: [&str; 2] = ["Yes", "No"];

/// The largest number a cell stores: `numeric(12,3)`, as
/// `values::parse_decimal` caps it.
const MAX_NUMBER: f64 = 999_999_999.999;

/// Whether a column must have a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Required {
    No,
    Yes,
    /// Required on new records of these kinds (their labels).
    For(Vec<String>),
}

/// One template column, as the workbook describes it.
#[derive(Debug, Clone)]
pub struct TemplateColumn {
    pub id: String,
    pub label: String,
    pub value_type: ValueType,
    pub required: Required,
    /// The dropdown's entries, by label; empty for a column without one.
    pub choices: Vec<String>,
    pub format: Option<String>,
}

/// The columns of a template with `headers`, each described by the
/// importer's field of that id. For a kind template (`kind`, assets), a
/// field answered per kind is read as that kind defines it, so its
/// dropdown holds that kind's options only. A header with no field is a
/// text column.
pub fn columns(
    conn: &mut DbConnection,
    importer: &dyn Importer,
    headers: &[String],
    kind: Option<&str>,
) -> Result<Vec<TemplateColumn>, diesel::result::Error> {
    let fields = importer.fields(conn)?;
    // Kinds named by `required_for` are slugs; the kind field's choices
    // label them.
    let kind_labels = match fields.iter().find(|f| f.id == "kind") {
        Some(field) => importer.value_choices(conn, field)?.unwrap_or_default(),
        None => Vec::new(),
    };
    let kind_label = |slug: &String| {
        kind_labels
            .iter()
            .find(|c| &c.value == slug)
            .map_or_else(|| slug.clone(), |c| c.label.clone())
    };

    let mut columns = Vec::with_capacity(headers.len());
    for id in headers {
        let Some(field) = fields.iter().find(|f| &f.id == id) else {
            columns.push(TemplateColumn {
                id: id.clone(),
                label: id.clone(),
                value_type: ValueType::Text,
                required: Required::No,
                choices: Vec::new(),
                format: None,
            });
            continue;
        };
        let own = match kind {
            Some(kind) => importer
                .value_contexts(conn, field)?
                .and_then(|contexts| contexts.into_iter().find(|c| c.key == kind))
                .map(|c| c.spec),
            None => None,
        };
        let spec = own.as_ref().unwrap_or(field);
        let required = if field.required {
            Required::Yes
        } else {
            match &field.required_for {
                Some(kinds) if !kinds.is_empty() => {
                    Required::For(kinds.iter().map(kind_label).collect())
                }
                _ => Required::No,
            }
        };
        columns.push(TemplateColumn {
            id: id.clone(),
            label: field.label.clone(),
            value_type: spec.value_type,
            required,
            choices: choices(conn, importer, spec)?,
            format: format_of(spec),
        });
    }
    Ok(columns)
}

/// The labels a column's dropdown offers: yes and no for a boolean, else
/// the entries Match values links the field's values to that can be
/// picked, and only the field's own options when it lists them (a role
/// the import refuses is a choice but not an option).
fn choices(
    conn: &mut DbConnection,
    importer: &dyn Importer,
    spec: &FieldSpec,
) -> Result<Vec<String>, diesel::result::Error> {
    if spec.value_type == ValueType::Boolean {
        return Ok(YES_NO.map(String::from).to_vec());
    }
    let Some(choices) = importer.value_choices(conn, spec)? else {
        return Ok(Vec::new());
    };
    let mut labels: Vec<String> = Vec::new();
    for choice in choices {
        let allowed = choice.disabled.is_none()
            && spec
                .enum_values
                .as_ref()
                .is_none_or(|values| values.contains(&choice.value));
        let label = choice.label.trim();
        if allowed && !label.is_empty() && !labels.iter().any(|l| l == label) {
            labels.push(label.to_string());
        }
    }
    Ok(labels)
}

fn format_of(spec: &FieldSpec) -> Option<String> {
    if let Some(format) = &spec.format {
        return Some(format.clone());
    }
    let format = match spec.value_type {
        ValueType::Date => "YYYY-MM-DD",
        ValueType::DateTime => "YYYY-MM-DD HH:MM",
        ValueType::Boolean => "Yes or No",
        ValueType::Array => "Separate items with ;",
        ValueType::User => "A member's email",
        ValueType::AssetRef => "An asset tag",
        _ => return None,
    };
    Some(format.to_string())
}

fn type_name(value_type: ValueType) -> &'static str {
    match value_type {
        ValueType::Text => "Text",
        ValueType::Enum => "Choice",
        ValueType::Number => "Number",
        ValueType::Date => "Date",
        ValueType::DateTime => "Date and time",
        ValueType::Boolean => "Yes or no",
        ValueType::Array => "List",
        ValueType::Email => "Email",
        ValueType::User => "Person",
        ValueType::AssetRef => "Asset",
    }
}

/// The workbook's bytes.
pub fn xlsx(columns: &[TemplateColumn]) -> Result<Vec<u8>, XlsxError> {
    let bold = Format::new().set_bold();
    let mut wb = Workbook::new();

    // Lists first, so each dropdown knows its range; the sheet goes last.
    let mut lists = Worksheet::new();
    lists.set_name(LISTS_SHEET)?.set_hidden(true);
    let mut ranges: Vec<Option<String>> = Vec::with_capacity(columns.len());
    let mut list_col: u16 = 0;
    for column in columns {
        if column.choices.is_empty() {
            ranges.push(None);
            continue;
        }
        lists.write_string_with_format(0, list_col, &column.id, &bold)?;
        for (i, label) in column.choices.iter().enumerate() {
            lists.write_string(i as u32 + 1, list_col, label)?;
        }
        let letter = super::xlsx::column_letter(u32::from(list_col));
        ranges.push(Some(format!(
            "={LISTS_SHEET}!${letter}$2:${letter}${}",
            column.choices.len() + 1
        )));
        list_col += 1;
    }

    let mut template = Worksheet::new();
    template.set_name(TEMPLATE_SHEET)?;
    let date = Format::new().set_num_format("yyyy-mm-dd");
    let date_time = Format::new().set_num_format("yyyy-mm-dd hh:mm");
    let last_row = MAX_ROWS as u32;
    for (col, (column, range)) in columns.iter().zip(&ranges).enumerate() {
        let col = col as u16;
        template.write_string_with_format(0, col, &column.id, &bold)?;
        template.set_column_width(col, (column.id.chars().count() + 4).clamp(12, 40) as f64)?;
        let validation = match (range, column.value_type) {
            (Some(range), _) => Some(
                DataValidation::new()
                    .allow_list_formula(Formula::new(range))
                    .set_error_title("Not in the list")?
                    .set_error_message(
                        "This value isn't one of the choices. Keep it to match it when you import.",
                    )?,
            ),
            (None, ValueType::Date) => {
                template.set_column_format(col, &date)?;
                Some(
                    DataValidation::new()
                        .allow_date(DataValidationRule::Between(
                            ExcelDateTime::from_ymd(1900, 1, 1)?,
                            ExcelDateTime::from_ymd(9999, 12, 31)?,
                        ))
                        .set_error_title("Not a date")?
                        .set_error_message("Enter a date as YYYY-MM-DD.")?,
                )
            }
            (None, ValueType::DateTime) => {
                template.set_column_format(col, &date_time)?;
                None
            }
            (None, ValueType::Number) => Some(
                DataValidation::new()
                    .allow_decimal_number(DataValidationRule::Between(-MAX_NUMBER, MAX_NUMBER))
                    .set_error_title("Not a number")?
                    .set_error_message("Enter a number.")?,
            ),
            _ => None,
        };
        if let Some(validation) = validation {
            let validation = validation.set_error_style(DataValidationErrorStyle::Warning);
            template.add_data_validation(1, col, last_row, col, &validation)?;
        }
    }
    template.set_freeze_panes(1, 0)?;

    let mut fields = Worksheet::new();
    fields.set_name(FIELDS_SHEET)?;
    let headings = [
        ("Column", 24.0),
        ("Label", 24.0),
        ("Type", 14.0),
        ("Required", 20.0),
        ("Allowed values", 60.0),
        ("Format", 22.0),
    ];
    for (col, (heading, width)) in headings.iter().enumerate() {
        fields.write_string_with_format(0, col as u16, *heading, &bold)?;
        fields.set_column_width(col as u16, *width)?;
    }
    for (i, column) in columns.iter().enumerate() {
        let row = i as u32 + 1;
        let required = match &column.required {
            Required::No => String::new(),
            Required::Yes => "Yes".to_string(),
            Required::For(kinds) => format!("For {}", kinds.join(", ")),
        };
        fields.write_string(row, 0, &column.id)?;
        fields.write_string(row, 1, &column.label)?;
        fields.write_string(row, 2, type_name(column.value_type))?;
        fields.write_string(row, 3, fit(required))?;
        fields.write_string(row, 4, fit(column.choices.join(", ")))?;
        fields.write_string(row, 5, column.format.as_deref().unwrap_or(""))?;
    }
    fields.set_freeze_panes(1, 0)?;

    // The Template sheet comes first: an upload opens on the first visible
    // sheet with rows.
    wb.push_worksheet(template);
    wb.push_worksheet(fields);
    wb.push_worksheet(lists);
    wb.save_to_buffer()
}

/// Text cut to what a cell holds.
fn fit(text: String) -> String {
    if text.chars().count() <= MAX_CELL_CHARS {
        return text;
    }
    text.chars().take(MAX_CELL_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(id: &str, value_type: ValueType, choices: &[&str]) -> TemplateColumn {
        TemplateColumn {
            id: id.to_string(),
            label: id.to_string(),
            value_type,
            required: Required::No,
            choices: choices.iter().map(ToString::to_string).collect(),
            format: None,
        }
    }

    fn sheet_xml(bytes: &[u8], part: &str) -> String {
        use std::io::Read;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
        let mut xml = String::new();
        archive
            .by_name(part)
            .expect("part")
            .read_to_string(&mut xml)
            .expect("read");
        xml
    }

    #[test]
    fn dropdowns_point_at_lists_and_dates_and_numbers_are_checked() {
        let bytes = xlsx(&[
            column("name", ValueType::Text, &[]),
            column("kind", ValueType::Enum, &["Camera", "=1+1"]),
            column("purchase_date", ValueType::Date, &[]),
            column("quantity", ValueType::Number, &[]),
            column("insured", ValueType::Boolean, &YES_NO),
        ])
        .expect("workbook");
        let template = sheet_xml(&bytes, "xl/worksheets/sheet1.xml");
        assert!(template.contains(r#"<pane ySplit="1""#), "{template}");
        assert_eq!(
            template.matches("<dataValidation ").count(),
            4,
            "{template}"
        );
        assert_eq!(template.matches(r#"errorStyle="warning""#).count(), 4);
        assert!(template.contains(r#"sqref="B2:B5001""#));
        assert!(template.contains("<formula1>Lists!$A$2:$A$3</formula1>"));
        assert!(template.contains("<formula1>Lists!$B$2:$B$3</formula1>"));
        assert!(template.contains(r#"type="date""#));
        assert!(template.contains(r#"type="decimal""#));
        // Nothing but the list ranges is a formula.
        for part in [
            "xl/worksheets/sheet1.xml",
            "xl/worksheets/sheet2.xml",
            "xl/worksheets/sheet3.xml",
        ] {
            assert!(!sheet_xml(&bytes, part).contains("<f>"), "{part}");
        }
    }
}
