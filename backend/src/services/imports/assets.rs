//! Asset CSV importer.
//!
//! Natural key: `asset_tag`. Rows with a tag matching an
//! existing asset upsert (UPDATE); rows with no tag, or with a
//! tag that doesn't match, INSERT. The empty-tag case is the
//! "first day, importing from a spreadsheet" path.
//!
//! Columns are universal-only: kind-specific attributes stay out
//! of the CSV, so a kind's required attributes aren't enforced on
//! import. The admin fills them in on the asset afterwards.
//!
//! An update that changes the kind must not leave the asset holding
//! attributes the new kind doesn't define. Such a row is an error; the
//! asset page, which shows what a kind change drops, is where to do it.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use bigdecimal::BigDecimal;
use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{Asset, AssetLifecycleEvent, AssetUpdate, NewAsset};
use crate::repository::asset_kinds as kind_repo;
use crate::repository::assets as asset_repo;

use super::csv_parser::ParsedCsv;
use super::types::{CommitError, ImportSummary, ImportedRecords, Importer, RowError, MAX_ERRORS};

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

        let lookups = Lookups::load(conn)?;
        let mut claims = FileClaims::default();

        for (i, row) in parsed.rows.iter().enumerate() {
            let row_num = i + 2; // header is row 1
            match plan_row(row, &lookups, &mut claims) {
                Ok(planned) => match planned.action {
                    RowAction::Create => summary.would_create += 1,
                    RowAction::Update(_) => summary.would_update += 1,
                },
                Err(field_errors) => {
                    for (column, message) in field_errors {
                        push_error(&mut summary, row_num, column, message);
                    }
                }
            }
        }

        Ok(summary)
    }

    /// Writes exactly the rows the dry-run counted: the same plan runs
    /// against the same lookups, and invalid rows are skipped.
    fn commit(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
    ) -> Result<ImportedRecords, CommitError> {
        if check_headers(&parsed.headers).is_some() {
            return Err(diesel::result::Error::QueryBuilderError(
                "header validation should have caught this; refusing to commit".into(),
            )
            .into());
        }
        let lookups = Lookups::load(conn)?;
        let mut claims = FileClaims::default();

        let mut assets = Vec::new();
        for (i, row) in parsed.rows.iter().enumerate() {
            let row_num = i + 2;
            let Ok(planned) = plan_row(row, &lookups, &mut claims) else {
                continue;
            };
            let written = match planned.action {
                RowAction::Create => {
                    asset_repo::create_device(conn, build_new_asset(row, planned.kind))
                }
                // Routed through update_device so the asset.updated sync
                // event fires, matching the create path.
                RowAction::Update(asset_id) => {
                    asset_repo::update_device(conn, asset_id, build_asset_update(row, planned.kind))
                }
            };
            assets.push(written.map_err(|e| row_write_error(row_num, e))?);
        }
        Ok(ImportedRecords::Assets(assets))
    }
}

#[derive(Debug, Clone, Copy)]
enum RowAction {
    Create,
    /// Update the existing asset with this id (matched on `asset_tag`).
    Update(i32),
}

struct PlannedRow {
    action: RowAction,
    /// The kind's slug, resolved from the slug or label in the file.
    kind: String,
}

/// Workspace state every row is checked against, loaded once per run.
struct Lookups {
    tags: HashMap<String, i32>,
    serials: HashMap<String, i32>,
    /// Kind and attributes of each tagged asset, the rows an update can hit.
    existing: HashMap<i32, (String, serde_json::Value)>,
    /// Lowercased slug and lowercased label, each to the kind's slug.
    kinds: HashMap<String, String>,
    kind_labels: Vec<String>,
    by_slug: HashMap<String, KindInfo>,
}

struct KindInfo {
    label: String,
    /// Attribute keys the kind's schema defines.
    properties: HashSet<String>,
}

impl Lookups {
    fn load(conn: &mut DbConnection) -> Result<Self, diesel::result::Error> {
        use crate::schema::assets;
        type Row = (
            i32,
            Option<String>,
            Option<String>,
            String,
            serde_json::Value,
        );
        let rows: Vec<Row> = assets::table
            .filter(
                assets::asset_tag
                    .is_not_null()
                    .or(assets::serial_number.is_not_null()),
            )
            .select((
                assets::id,
                assets::asset_tag,
                assets::serial_number,
                assets::kind,
                assets::attributes,
            ))
            .load(conn)?;
        let mut tags = HashMap::new();
        let mut serials = HashMap::new();
        let mut existing = HashMap::new();
        for (id, tag, serial, kind, attributes) in rows {
            if let Some(tag) = tag {
                tags.insert(tag, id);
                existing.insert(id, (kind, attributes));
            }
            if let Some(serial) = serial {
                serials.insert(serial, id);
            }
        }

        let registry = kind_repo::list_kinds(conn)?;
        let mut kinds = HashMap::new();
        // Slugs first so a label never shadows another kind's slug.
        for k in &registry {
            kinds.insert(k.slug.to_lowercase(), k.slug.clone());
        }
        for k in &registry {
            kinds
                .entry(k.label.to_lowercase())
                .or_insert_with(|| k.slug.clone());
        }
        let kind_labels = registry.iter().map(|k| k.label.clone()).collect();
        let by_slug = registry
            .into_iter()
            .map(|k| {
                let properties = k
                    .attribute_schema
                    .get("properties")
                    .and_then(serde_json::Value::as_object)
                    .map(|props| props.keys().cloned().collect())
                    .unwrap_or_default();
                let info = KindInfo {
                    label: k.label,
                    properties,
                };
                (k.slug, info)
            })
            .collect();

        Ok(Self {
            tags,
            serials,
            existing,
            kinds,
            kind_labels,
            by_slug,
        })
    }
}

/// Tags and serials claimed by earlier valid rows of the same file. Only
/// a row that passes every check claims its values, so an invalid row
/// never blocks a later valid one.
#[derive(Default)]
struct FileClaims {
    tags: HashSet<String>,
    serials: HashSet<String>,
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

/// Column widths of the `assets` table (varchar limits).
const MAX_LENGTHS: &[(&str, usize)] = &[
    ("name", 255),
    ("asset_tag", 255),
    ("serial_number", 255),
    ("manufacturer", 255),
    ("model", 255),
    ("location", 255),
    ("unit", 32),
];

/// Largest magnitude `numeric(12,3)` holds once rounded to 3 places.
const MAX_DECIMAL: &str = "999999999.9995";

/// Check one row. On success returns what the commit will do with it; on
/// failure returns the per-field errors. The dry-run and the commit both
/// call this, in file order, so they agree on every row.
fn plan_row(
    row: &HashMap<String, String>,
    lookups: &Lookups,
    claims: &mut FileClaims,
) -> Result<PlannedRow, Vec<(Option<String>, String)>> {
    let mut errors: Vec<(Option<String>, String)> = Vec::new();
    let mut error =
        |column: &str, message: String| errors.push((Some(column.to_string()), message));

    if trimmed(row, "name").is_empty() {
        error("name", "name is required".to_string());
    }

    for (column, max) in MAX_LENGTHS {
        let len = trimmed(row, column).chars().count();
        if len > *max {
            error(column, format!("longer than {max} characters ({len})"));
        }
    }

    // Matched on slug or label, ignoring case. Kind attributes aren't
    // CSV columns, so the kind's required attributes aren't checked
    // here: the asset is created without them and filled in on its page.
    let kind_cell = trimmed(row, "kind");
    let kind = if kind_cell.is_empty() {
        error("kind", "kind is required".to_string());
        None
    } else {
        let found = lookups.kinds.get(&kind_cell.to_lowercase()).cloned();
        if found.is_none() {
            error(
                "kind",
                format!(
                    "unknown kind '{kind_cell}'. Use one of: {}, or add it under Admin → Asset Kinds",
                    lookups.kind_labels.join(", ")
                ),
            );
        }
        found
    };

    if let Err(message) = check_decimal(row, "quantity") {
        error("quantity", message);
    }
    match check_decimal(row, "low_stock_threshold") {
        Err(message) => error("low_stock_threshold", message),
        Ok(Some(v)) if v < 0 => error("low_stock_threshold", "must be 0 or more".to_string()),
        Ok(_) => {}
    }

    let tag = trimmed(row, "asset_tag");
    let target = if tag.is_empty() {
        None
    } else {
        lookups.tags.get(&tag).copied()
    };
    if !tag.is_empty() && claims.tags.contains(&tag) {
        error(
            "asset_tag",
            format!("tag '{tag}' appears more than once in this file"),
        );
    }

    // Same canonical form the repository stores (trimmed, blank is none).
    let serial = trimmed(row, "serial_number");
    if !serial.is_empty() {
        if claims.serials.contains(&serial) {
            error(
                "serial_number",
                format!("serial number '{serial}' appears more than once in this file"),
            );
        } else if let Some(holder) = lookups.serials.get(&serial) {
            if Some(*holder) != target {
                error(
                    "serial_number",
                    format!("serial number '{serial}' is already used by another asset"),
                );
            }
        }
    }

    // A kind change must not strand attributes the new kind doesn't
    // define. Required attributes stay unchecked, as for new rows.
    if let (Some(new_kind), Some(id)) = (&kind, target) {
        if let Some(message) = kind_change_error(lookups, id, new_kind) {
            error("kind", message);
        }
    }

    match kind {
        Some(kind) if errors.is_empty() => {
            if !tag.is_empty() {
                claims.tags.insert(tag);
            }
            if !serial.is_empty() {
                claims.serials.insert(serial);
            }
            let action = match target {
                Some(id) => RowAction::Update(id),
                None => RowAction::Create,
            };
            Ok(PlannedRow { action, kind })
        }
        _ => Err(errors),
    }
}

/// The error for an update that moves asset `id` to `new_kind` while it
/// holds attributes the new kind doesn't define, if it does.
fn kind_change_error(lookups: &Lookups, id: i32, new_kind: &str) -> Option<String> {
    let (old_kind, attributes) = lookups.existing.get(&id)?;
    if old_kind == new_kind {
        return None;
    }
    let defined = lookups.by_slug.get(new_kind).map(|k| &k.properties);
    let mut dropped: Vec<&str> = attributes
        .as_object()?
        .keys()
        .filter(|key| !defined.is_some_and(|d| d.contains(*key)))
        .map(String::as_str)
        .collect();
    if dropped.is_empty() {
        return None;
    }
    dropped.sort_unstable();
    let label = |slug: &str| {
        lookups
            .by_slug
            .get(slug)
            .map_or_else(|| slug.to_string(), |k| k.label.clone())
    };
    Some(format!(
        "changing kind from {} to {} would drop attributes: {}; change the kind on the asset page instead",
        label(old_kind),
        label(new_kind),
        dropped.join(", ")
    ))
}

/// A row the database refused at commit, named for the admin. Anything
/// that isn't about the row's values stays a database error.
fn row_write_error(row: usize, e: diesel::result::Error) -> CommitError {
    use diesel::result::{DatabaseErrorKind, Error};
    let message = match &e {
        Error::DatabaseError(DatabaseErrorKind::UniqueViolation, info)
            if info.constraint_name() == Some("idx_asset_serial_unique") =>
        {
            "its serial number is already used by another asset".to_string()
        }
        Error::DatabaseError(
            DatabaseErrorKind::UniqueViolation
            | DatabaseErrorKind::CheckViolation
            | DatabaseErrorKind::NotNullViolation
            | DatabaseErrorKind::Unknown,
            info,
        ) => info.message().to_string(),
        _ => return CommitError::Db(e),
    };
    CommitError::Row { row, message }
}

fn trimmed(row: &HashMap<String, String>, key: &str) -> String {
    row.get(key).cloned().unwrap_or_default().trim().to_string()
}

/// A decimal cell: `Ok(None)` when blank, the value when it parses and
/// fits `numeric(12,3)`, otherwise the message for the admin.
fn check_decimal(row: &HashMap<String, String>, key: &str) -> Result<Option<BigDecimal>, String> {
    let raw = trimmed(row, key);
    if raw.is_empty() {
        return Ok(None);
    }
    let value =
        BigDecimal::from_str(&raw).map_err(|_| format!("'{raw}' is not a valid decimal"))?;
    let max = BigDecimal::from_str(MAX_DECIMAL).expect("constant parses");
    if value.abs() >= max {
        return Err(format!("'{raw}' is too large (at most 999999999.999)"));
    }
    Ok(Some(value))
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

fn build_new_asset(row: &HashMap<String, String>, kind: String) -> NewAsset {
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
        kind,
        attributes: serde_json::json!({}),
        quantity: opt_decimal(row, "quantity"),
        unit: opt_string(row, "unit"),
        external_sync_source: None,
        low_stock_threshold: opt_decimal(row, "low_stock_threshold"),
    }
}

fn build_asset_update(row: &HashMap<String, String>, kind: String) -> AssetUpdate {
    // A blank cell leaves the column as it is (outer `None`); it never clears.
    AssetUpdate {
        name: Some(trimmed(row, "name")),
        serial_number: opt_string(row, "serial_number").map(Some),
        manufacturer: opt_string(row, "manufacturer"),
        model: opt_string(row, "model"),
        location: opt_string(row, "location").map(Some),
        notes: opt_string(row, "notes").map(Some),
        primary_user_uuid: None,
        managed_by_user_uuid: None,
        purchase_date: None,
        asset_tag: opt_string(row, "asset_tag").map(Some),
        updated_at: Some(chrono::Utc::now().naive_utc()),
        kind: Some(kind),
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

/// A lifecycle-history export row: an event enriched with its asset's tag +
/// name. Shared by the CSV and JSON exports. Actor + ticket stay as ids (the
/// ticket id is the correlation handle); `metadata` is the raw JSON.
#[derive(serde::Serialize)]
pub struct AssetHistoryExportRow {
    pub asset_tag: Option<String>,
    pub asset_name: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub from_status: Option<String>,
    pub to_status: String,
    pub reason: Option<String>,
    pub actor_uuid: Option<uuid::Uuid>,
    pub ticket_id: Option<i32>,
    pub metadata: serde_json::Value,
}

/// Enrich lifecycle events with their asset's tag + name, in the given order.
/// The single source for both the CSV and JSON history exports.
pub fn build_history_rows(
    assets: &[Asset],
    events: &[AssetLifecycleEvent],
) -> Vec<AssetHistoryExportRow> {
    let by_id: HashMap<i32, &Asset> = assets.iter().map(|a| (a.id, a)).collect();
    events
        .iter()
        .map(|e| {
            let asset = by_id.get(&e.asset_id);
            AssetHistoryExportRow {
                asset_tag: asset.and_then(|a| a.asset_tag.clone()),
                asset_name: asset.map(|a| a.name.clone()).unwrap_or_default(),
                occurred_at: e.occurred_at,
                from_status: e.from_status.clone(),
                to_status: e.to_status.clone(),
                reason: e.reason.clone(),
                actor_uuid: e.actor_uuid,
                ticket_id: e.ticket_id,
                metadata: e.metadata.clone(),
            }
        })
        .collect()
}

/// CSV of lifecycle history (one row per event). Mirrors `write_assets_csv`;
/// shares `build_history_rows` with the JSON export.
pub fn write_history_csv(
    assets: &[Asset],
    events: &[AssetLifecycleEvent],
) -> Result<Vec<u8>, csv::Error> {
    let rows = build_history_rows(assets, events);
    let mut buf = Vec::new();
    {
        let mut wtr = csv::Writer::from_writer(&mut buf);
        wtr.write_record(HISTORY_HEADERS)?;
        for r in &rows {
            wtr.write_record([
                r.asset_tag.clone().unwrap_or_default(),
                r.asset_name.clone(),
                r.occurred_at.to_rfc3339(),
                r.from_status.clone().unwrap_or_default(),
                r.to_status.clone(),
                r.reason.clone().unwrap_or_default(),
                r.actor_uuid.map(|u| u.to_string()).unwrap_or_default(),
                r.ticket_id.map(|t| t.to_string()).unwrap_or_default(),
                r.metadata.to_string(),
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
