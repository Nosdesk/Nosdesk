//! Asset CSV importer.
//!
//! Natural key: `asset_tag`. Rows with a tag matching an
//! existing asset upsert (UPDATE); rows with no tag, or with a
//! tag that doesn't match, INSERT. The empty-tag case is the
//! "first day, importing from a spreadsheet" path.
//!
//! Columns are universal-only for Phase 1: kind-specific
//! attributes stay out of the CSV. The admin can edit them
//! through the per-kind attribute form after the bulk import.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use bigdecimal::BigDecimal;
use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{Asset, AssetLifecycleEvent, AssetUpdate, NewAsset};
use crate::repository::asset_kinds as kind_repo;
use crate::repository::assets as asset_repo;
use crate::services::assets::kinds as schema_validator;

use super::csv_parser::ParsedCsv;
use super::types::{ImportSummary, ImportedRecords, Importer, RowError, MAX_ERRORS};

/// Universal CSV columns the importer accepts. Export writes
/// these first (same order/names) so a round-trip upserts on
/// `asset_tag` without column drift.
pub const IMPORT_HEADERS: &[&str] = &[
    "name",
    "kind",
    "asset_tag",
    "serial_number",
    "manufacturer",
    "model",
    "location",
    "notes",
    "quantity",
    "unit",
    "low_stock_threshold",
];

/// Columns appended on export only. The importer ignores
/// extras, so re-importing an unmodified export is a no-op
/// upsert on the shared columns.
pub const EXPORT_EXTRA_HEADERS: &[&str] = &["status", "attributes"];

pub struct AssetImporter;

impl Importer for AssetImporter {
    fn template_headers(&self) -> &'static [&'static str] {
        IMPORT_HEADERS
    }

    fn dry_run(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
    ) -> Result<ImportSummary, diesel::result::Error> {
        let mut summary = ImportSummary {
            row_count: parsed.rows.len(),
            would_create: 0,
            would_update: 0,
            errors: Vec::new(),
            errors_truncated: false,
        };

        if let Some(err) = check_headers(&parsed.headers) {
            push_error(&mut summary, 1, None, err);
            return Ok(summary);
        }

        // Load existing tags + the kind registry up front. Both
        // are small (asset_tag is human-typed and bounded;
        // asset_kinds is a workspace registry of ~10 rows).
        let existing_tags = load_existing_tags(conn)?;
        let known_kinds = load_known_kinds(conn)?;

        // Detect duplicate tags within the same upload. The
        // first occurrence "wins" for the would-create/update
        // count; subsequent occurrences raise an error so the
        // admin spots the dup before committing.
        let mut tags_in_file: HashSet<String> = HashSet::new();

        for (i, row) in parsed.rows.iter().enumerate() {
            let row_num = i + 2; // header is row 1
            match validate_row(row, &known_kinds, &existing_tags, &mut tags_in_file) {
                Ok(RowAction::Create) => summary.would_create += 1,
                Ok(RowAction::Update) => summary.would_update += 1,
                Err(field_errors) => {
                    for (column, message) in field_errors {
                        push_error(&mut summary, row_num, column, message);
                    }
                }
            }
        }

        Ok(summary)
    }

    fn commit(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
    ) -> Result<ImportedRecords, diesel::result::Error> {
        if check_headers(&parsed.headers).is_some() {
            return Err(diesel::result::Error::QueryBuilderError(
                "header validation should have caught this; refusing to commit".into(),
            ));
        }
        let known_kinds = load_known_kinds(conn)?;
        let existing_tags = load_existing_tags(conn)?;
        let mut tags_in_file: HashSet<String> = HashSet::new();

        let mut assets = Vec::new();
        for row in &parsed.rows {
            // Re-validate; the dry-run was advisory but the
            // commit guards against changes between phases.
            let mut local_tags = tags_in_file.clone();
            let action = match validate_row(row, &known_kinds, &existing_tags, &mut local_tags) {
                Ok(a) => a,
                Err(_) => continue, // skip invalid rows
            };
            // Sync the dedup set on success only so an invalid
            // duplicate tag doesn't block the legitimate first
            // occurrence (already handled inside validate_row).
            tags_in_file = local_tags;

            match action {
                RowAction::Create => {
                    let new = build_new_asset(row);
                    let asset = asset_repo::create_device(conn, new)?;
                    assets.push(asset);
                }
                RowAction::Update => {
                    let tag = row.get("asset_tag").map(String::as_str).unwrap_or("");
                    let asset_id = match existing_tags.get(tag) {
                        Some(id) => *id,
                        None => continue,
                    };
                    let update = build_asset_update(row);
                    // Route updates through update_device so the
                    // asset.updated sync event fires, matching the
                    // create path (create_device) and the rest of the
                    // app instead of a silent raw UPDATE.
                    let asset = asset_repo::update_device(conn, asset_id, update)?;
                    assets.push(asset);
                }
            }
        }
        Ok(ImportedRecords::Assets(assets))
    }
}

#[derive(Debug, Clone, Copy)]
enum RowAction {
    Create,
    Update,
}

fn check_headers(headers: &[String]) -> Option<String> {
    let expected: HashSet<&str> = IMPORT_HEADERS.iter().copied().collect();
    let provided: HashSet<&str> = headers.iter().map(String::as_str).collect();
    let missing: Vec<&&str> = expected.difference(&provided).collect();
    if !missing.is_empty() {
        let names: Vec<String> = missing.iter().map(|s| (**s).to_string()).collect();
        return Some(format!("missing required columns: {}", names.join(", ")));
    }
    None
}

fn load_existing_tags(
    conn: &mut DbConnection,
) -> Result<HashMap<String, i32>, diesel::result::Error> {
    use crate::schema::assets;
    let rows: Vec<(i32, Option<String>)> = assets::table
        .filter(assets::asset_tag.is_not_null())
        .select((assets::id, assets::asset_tag))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, tag)| tag.map(|t| (t, id)))
        .collect())
}

fn load_known_kinds(
    conn: &mut DbConnection,
) -> Result<HashMap<String, serde_json::Value>, diesel::result::Error> {
    let kinds = kind_repo::list_kinds(conn)?;
    Ok(kinds
        .into_iter()
        .map(|k| (k.slug, k.attribute_schema))
        .collect())
}

fn push_error(summary: &mut ImportSummary, row: usize, column: Option<String>, message: String) {
    if summary.errors.len() >= MAX_ERRORS {
        summary.errors_truncated = true;
        return;
    }
    summary.errors.push(RowError {
        row,
        column,
        message,
    });
}

/// Validate one row. On success returns whether the row would
/// create or update; on failure returns the per-field errors.
fn validate_row(
    row: &HashMap<String, String>,
    known_kinds: &HashMap<String, serde_json::Value>,
    existing_tags: &HashMap<String, i32>,
    tags_in_file: &mut HashSet<String>,
) -> Result<RowAction, Vec<(Option<String>, String)>> {
    let mut errors: Vec<(Option<String>, String)> = Vec::new();

    let name = trimmed(row, "name");
    if name.is_empty() {
        errors.push((Some("name".to_string()), "name is required".to_string()));
    }

    let kind = trimmed(row, "kind");
    if kind.is_empty() {
        errors.push((Some("kind".to_string()), "kind is required".to_string()));
    } else if !known_kinds.contains_key(&kind) {
        errors.push((
            Some("kind".to_string()),
            format!("unknown kind '{kind}'; create it under Admin → Asset Kinds first"),
        ));
    }

    if let Some(message) = parse_decimal(row, "quantity") {
        errors.push((Some("quantity".to_string()), message));
    }
    if let Some(message) = parse_decimal(row, "low_stock_threshold") {
        errors.push((Some("low_stock_threshold".to_string()), message));
    }

    // Empty attributes are fine for Phase 1; we don't allow the
    // CSV to carry kind-specific attribute columns yet, so the
    // attribute_schema check uses an empty object.
    if let Some(schema) = known_kinds.get(&kind) {
        if let Err(e) = schema_validator::validate_attributes(schema, &serde_json::json!({})) {
            errors.push((Some("kind".to_string()), e.to_string()));
        }
    }

    let tag = trimmed(row, "asset_tag");
    let action = if tag.is_empty() {
        RowAction::Create
    } else {
        if !tags_in_file.insert(tag.clone()) {
            errors.push((
                Some("asset_tag".to_string()),
                format!("tag '{tag}' appears more than once in this file"),
            ));
        }
        if existing_tags.contains_key(&tag) {
            RowAction::Update
        } else {
            RowAction::Create
        }
    };

    if errors.is_empty() {
        Ok(action)
    } else {
        Err(errors)
    }
}

fn trimmed(row: &HashMap<String, String>, key: &str) -> String {
    row.get(key).cloned().unwrap_or_default().trim().to_string()
}

fn parse_decimal(row: &HashMap<String, String>, key: &str) -> Option<String> {
    let raw = trimmed(row, key);
    if raw.is_empty() {
        return None;
    }
    match BigDecimal::from_str(&raw) {
        Ok(_) => None,
        Err(_) => Some(format!("'{raw}' is not a valid decimal")),
    }
}

fn opt_string(row: &HashMap<String, String>, key: &str) -> Option<String> {
    let v = trimmed(row, key);
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

fn opt_decimal(row: &HashMap<String, String>, key: &str) -> Option<BigDecimal> {
    let v = trimmed(row, key);
    if v.is_empty() {
        None
    } else {
        BigDecimal::from_str(&v).ok()
    }
}

fn build_new_asset(row: &HashMap<String, String>) -> NewAsset {
    NewAsset {
        name: trimmed(row, "name"),
        serial_number: opt_string(row, "serial_number"),
        manufacturer: opt_string(row, "manufacturer"),
        model: opt_string(row, "model"),
        location: opt_string(row, "location"),
        notes: opt_string(row, "notes"),
        primary_user_uuid: None,
        purchase_date: None,
        asset_tag: opt_string(row, "asset_tag"),
        kind: trimmed(row, "kind"),
        attributes: serde_json::json!({}),
        quantity: opt_decimal(row, "quantity"),
        unit: opt_string(row, "unit"),
        external_sync_source: None,
        low_stock_threshold: opt_decimal(row, "low_stock_threshold"),
    }
}

fn build_asset_update(row: &HashMap<String, String>) -> AssetUpdate {
    AssetUpdate {
        name: Some(trimmed(row, "name")),
        serial_number: opt_string(row, "serial_number"),
        manufacturer: opt_string(row, "manufacturer"),
        model: opt_string(row, "model"),
        location: opt_string(row, "location"),
        notes: opt_string(row, "notes"),
        primary_user_uuid: None,
        managed_by_user_uuid: None,
        purchase_date: None,
        asset_tag: opt_string(row, "asset_tag"),
        updated_at: Some(chrono::Utc::now().naive_utc()),
        kind: Some(trimmed(row, "kind")),
        attributes: None,
        quantity: Some(opt_decimal(row, "quantity")),
        unit: Some(opt_string(row, "unit")),
        external_sync_source: None,
        low_stock_threshold: Some(opt_decimal(row, "low_stock_threshold")),
        // CSV import doesn't link the model catalog (yet); leave the
        // existing model link untouched.
        model_id: None,
    }
}

fn opt_decimal_string(value: &Option<bigdecimal::BigDecimal>) -> String {
    value.as_ref().map(ToString::to_string).unwrap_or_default()
}

fn opt_string_field(value: &Option<String>) -> String {
    value.as_deref().unwrap_or("").to_string()
}

/// Neutralize spreadsheet formula injection. Excel and Sheets treat a
/// cell as a formula when its first non-whitespace character is `=`,
/// `+`, `-`, or `@`. Some importers strip leading whitespace (space,
/// tab, CR, LF, NUL) before evaluating, so a payload like "\t=cmd"
/// would slip past a first-byte-only check; look past the leading
/// whitespace before deciding. Prefix with `'` so the value opens as
/// plain text. The `csv` crate already quotes commas and newlines;
/// this is separate hardening. See security-audit-2026-06.
fn csv_formula_safe(value: String) -> String {
    let leading = value.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{0}');
    match leading.chars().next() {
        Some('=' | '+' | '-' | '@') => format!("'{value}"),
        _ => value,
    }
}

/// One CSV data row for an asset. Column order matches
/// `export_header_row`.
fn asset_export_row(asset: &Asset) -> Vec<String> {
    [
        asset.name.clone(),
        asset.kind.clone(),
        opt_string_field(&asset.asset_tag),
        opt_string_field(&asset.serial_number),
        opt_string_field(&asset.manufacturer),
        opt_string_field(&asset.model),
        opt_string_field(&asset.location),
        opt_string_field(&asset.notes),
        opt_decimal_string(&asset.quantity),
        opt_string_field(&asset.unit),
        opt_decimal_string(&asset.low_stock_threshold),
        asset.status.clone(),
        asset.attributes.to_string(),
    ]
    .into_iter()
    .map(csv_formula_safe)
    .collect()
}

fn export_header_row() -> Vec<&'static str> {
    IMPORT_HEADERS
        .iter()
        .chain(EXPORT_EXTRA_HEADERS.iter())
        .copied()
        .collect()
}

/// Serialize workspace assets to CSV bytes. Headers match the
/// importer columns plus `status` and `attributes`.
///
/// v1 buffers the full export in memory (`Vec<u8>`), same tradeoff
/// as the import pipeline. Streaming can replace this if workspace
/// asset counts outgrow RAM.
pub fn write_assets_csv(assets: &[Asset]) -> Result<Vec<u8>, csv::Error> {
    let mut buf = Vec::new();
    {
        let mut wtr = csv::Writer::from_writer(&mut buf);
        wtr.write_record(export_header_row())?;
        for asset in assets {
            wtr.write_record(asset_export_row(asset))?;
        }
        wtr.flush()?;
    }
    Ok(buf)
}

/// Columns of the lifecycle-history export. One row per event.
const HISTORY_HEADERS: &[&str] = &[
    "asset_tag",
    "asset_name",
    "occurred_at",
    "from_status",
    "to_status",
    "reason",
    "actor_uuid",
    "ticket_id",
    "metadata",
];

/// CSV of lifecycle history: one row per event, enriched with its asset's tag +
/// name (looked up from `assets`). Actor + ticket are ids (the ticket id is the
/// correlation handle); `metadata` is the raw JSON. Mirrors `write_assets_csv`.
pub fn write_history_csv(
    assets: &[Asset],
    events: &[AssetLifecycleEvent],
) -> Result<Vec<u8>, csv::Error> {
    let by_id: HashMap<i32, &Asset> = assets.iter().map(|a| (a.id, a)).collect();
    let mut buf = Vec::new();
    {
        let mut wtr = csv::Writer::from_writer(&mut buf);
        wtr.write_record(HISTORY_HEADERS)?;
        for e in events {
            let asset = by_id.get(&e.asset_id);
            wtr.write_record([
                asset.and_then(|a| a.asset_tag.clone()).unwrap_or_default(),
                asset.map(|a| a.name.clone()).unwrap_or_default(),
                e.occurred_at.to_rfc3339(),
                e.from_status.clone().unwrap_or_default(),
                e.to_status.clone(),
                e.reason.clone().unwrap_or_default(),
                e.actor_uuid.map(|u| u.to_string()).unwrap_or_default(),
                e.ticket_id.map(|t| t.to_string()).unwrap_or_default(),
                e.metadata.to_string(),
            ])?;
        }
        wtr.flush()?;
    }
    Ok(buf)
}

#[cfg(test)]
mod export_tests {
    use super::*;
    use bigdecimal::BigDecimal;
    use chrono::NaiveDateTime;
    use serde_json::json;
    use std::str::FromStr;

    fn sample_asset() -> Asset {
        Asset {
            id: 1,
            name: "Laptop".to_string(),
            serial_number: Some("SN-1".to_string()),
            manufacturer: Some("Acme".to_string()),
            model: Some("X1".to_string()),
            location: Some("Lab".to_string()),
            created_at: NaiveDateTime::parse_from_str("2024-01-01 00:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            updated_at: NaiveDateTime::parse_from_str("2024-01-01 00:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            created_by: None,
            notes: Some("note".to_string()),
            primary_user_uuid: None,
            purchase_date: None,
            asset_tag: Some("TAG-1".to_string()),
            kind: "device".to_string(),
            attributes: json!({"hostname":"pc-1"}),
            quantity: Some(BigDecimal::from_str("12.5").unwrap()),
            unit: Some("pcs".to_string()),
            external_sync_source: None,
            low_stock_threshold: Some(BigDecimal::from_str("2").unwrap()),
            workspace_id: 1,
            status: "in_service".to_string(),
            model_id: None,
            managed_by_user_uuid: None,
        }
    }

    #[test]
    fn export_csv_headers_match_importer_plus_extras() {
        let csv = write_assets_csv(&[sample_asset()]).unwrap();
        let text = String::from_utf8(csv).unwrap();
        let header = text.lines().next().unwrap();
        assert_eq!(
            header,
            "name,kind,asset_tag,serial_number,manufacturer,model,location,notes,quantity,unit,low_stock_threshold,status,attributes"
        );
    }

    #[test]
    fn history_csv_header_and_row() {
        let asset = sample_asset();
        let occurred_at: chrono::DateTime<chrono::Utc> = "2024-02-01T00:00:00Z".parse().unwrap();
        let event = AssetLifecycleEvent {
            id: 1,
            asset_id: 1,
            from_status: Some("in_service".to_string()),
            to_status: "in_repair".to_string(),
            reason: Some("screen".to_string()),
            ticket_id: Some(42),
            metadata: json!({"vendor": "Acme"}),
            actor_uuid: None,
            occurred_at,
            workspace_id: 1,
        };
        let csv = write_history_csv(&[asset], &[event]).unwrap();
        let text = String::from_utf8(csv).unwrap();
        let mut lines = text.lines();
        assert_eq!(
            lines.next().unwrap(),
            "asset_tag,asset_name,occurred_at,from_status,to_status,reason,actor_uuid,ticket_id,metadata"
        );
        let row = lines.next().unwrap();
        assert!(row.starts_with("TAG-1,Laptop,"));
        // reason, empty actor, ticket id in order (the ticket id is the correlation).
        assert!(row.contains(",in_service,in_repair,screen,,42,"));
    }

    #[test]
    fn export_prefixes_formula_trigger_cells() {
        let mut asset = sample_asset();
        asset.name = "=SUM(A1)".to_string();
        asset.notes = Some("+cmd".to_string());
        let csv = write_assets_csv(&[asset]).unwrap();
        let text = String::from_utf8(csv).unwrap();
        let row = text.lines().nth(1).unwrap();
        assert!(row.starts_with("'=SUM(A1)"));
        assert!(row.contains(",'+cmd,"));
    }

    #[test]
    fn formula_guard_handles_leading_whitespace_and_controls() {
        // Direct triggers.
        assert_eq!(csv_formula_safe("=cmd".into()), "'=cmd");
        assert_eq!(csv_formula_safe("@SUM".into()), "'@SUM");
        // Leading whitespace / control chars before the trigger must
        // still be neutralised (some importers strip them first).
        assert_eq!(csv_formula_safe(" =cmd".into()), "' =cmd");
        assert_eq!(csv_formula_safe("\t=cmd".into()), "'\t=cmd");
        assert_eq!(csv_formula_safe("\r-cmd".into()), "'\r-cmd");
        // Benign values are untouched.
        assert_eq!(csv_formula_safe("Laptop".into()), "Laptop");
        assert_eq!(csv_formula_safe("  hello".into()), "  hello");
        assert_eq!(csv_formula_safe(String::new()), "");
    }
}
