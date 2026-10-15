//! Asset CSV importer.
//!
//! Natural key: `asset_tag` by default, or `serial_number`, as the
//! admin picks at the Map step (`ImportOptions::match_by`). Rows whose
//! key matches an existing asset upsert (UPDATE); rows with no key, or a
//! key that doesn't match, INSERT. The empty-key case is the "first day,
//! importing from a spreadsheet" path. A tag more than one asset holds is
//! a row error, and so is a tag or serial that more than one row of the
//! file holds: every such row is skipped, so the admin picks the right
//! one. Matched by serial, a tag another asset holds is an error, so an
//! import never leaves two assets sharing a tag. With matching off, every
//! row creates and a row whose tag or serial exists is an error. A blank cell never clears a column on update; a
//! blank kind keeps the asset's kind, and on a new asset takes the default
//! kind when one is set.
//!
//! Columns are universal-only: kind-specific attributes stay out
//! of the CSV, so a kind's required attributes aren't enforced on
//! import. The admin fills them in on the asset afterwards.
//!
//! An update writes only what `PUT /assets/{id}` and the asset page would.
//! An asset synced from Microsoft takes no column from the file: its row
//! must match what's stored. An asset linked to a model keeps its kind,
//! since the model sets it. A kind change never leaves the asset holding
//! attributes the new kind doesn't define. Any other row is an error that
//! points to the asset page.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use bigdecimal::BigDecimal;
use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{Asset, AssetLifecycleEvent, AssetUpdate, NewAsset};
use crate::repository::asset_kinds as kind_repo;
use crate::repository::asset_models as model_repo;
use crate::repository::assets as asset_repo;

use super::csv_parser::ParsedCsv;
use super::safe_cells::csv_formula_safe;
use super::types::{
    duplicate_in_file, CommitError, Committed, DecimalMark, FieldGroup, FieldSpec, ImportContext,
    ImportOptions, ImportType, ImportedRecords, Importer, MatchBy, Notice, RecordId, RowOutcome,
    RowPlan, RowResult, ValueType,
};

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

    fn fields(&self, conn: &mut DbConnection) -> Result<Vec<FieldSpec>, diesel::result::Error> {
        use FieldGroup::{Core, Links};
        use ValueType::{Enum, Number, Text};
        let t = ImportType::Assets;
        let kinds = kind_repo::list_kinds(conn)?
            .into_iter()
            .map(|k| k.slug)
            .collect();
        Ok(vec![
            FieldSpec::fixed(t, "name", "Name", Core, Text, true),
            FieldSpec::fixed(t, "kind", "Kind", Core, Enum, true).with_enum(kinds),
            FieldSpec::fixed(t, "asset_tag", "Asset tag", Core, Text, false),
            FieldSpec::fixed(t, "serial_number", "Serial number", Core, Text, false),
            FieldSpec::fixed(t, "location", "Location", Core, Text, false),
            FieldSpec::fixed(t, "notes", "Notes", Core, Text, false),
            FieldSpec::fixed(t, "quantity", "Quantity", Core, Number, false),
            FieldSpec::fixed(t, "unit", "Unit", Core, Text, false),
            FieldSpec::fixed(
                t,
                "low_stock_threshold",
                "Low stock threshold",
                Core,
                Number,
                false,
            ),
            FieldSpec::fixed(t, "manufacturer", "Manufacturer", Links, Text, false),
            FieldSpec::fixed(t, "model", "Model", Links, Text, false),
        ])
    }

    fn synonyms(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("assetname", "name"),
            ("devicename", "name"),
            ("itemname", "name"),
            ("type", "kind"),
            ("assettype", "kind"),
            ("tag", "asset_tag"),
            ("tagno", "asset_tag"),
            ("assetno", "asset_tag"),
            ("assetnumber", "asset_tag"),
            ("serial", "serial_number"),
            ("sn", "serial_number"),
            ("serialno", "serial_number"),
            ("make", "manufacturer"),
            ("brand", "manufacturer"),
            ("modelno", "model"),
            ("modelnumber", "model"),
            ("room", "location"),
            ("site", "location"),
            ("comments", "notes"),
            ("comment", "notes"),
            ("note", "notes"),
            ("qty", "quantity"),
        ]
    }

    fn dry_run(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        ctx: &ImportContext,
    ) -> Result<RowPlan, diesel::result::Error> {
        if let Some(err) = check_headers(&parsed.headers) {
            return Ok(RowPlan {
                file_error: Some(err),
                ..Default::default()
            });
        }

        let lookups = Lookups::load(conn)?;
        let file_keys = FileKeys::scan(parsed);
        let mut plan = RowPlan::default();
        let mut keyless = 0;

        for record in parsed.records() {
            let row = &record.values;
            let result = match plan_row(row, &lookups, &file_keys, &ctx.options) {
                Ok(planned) => match planned.action {
                    RowAction::Create => {
                        if is_keyless(row) {
                            keyless += 1;
                        }
                        RowResult::planned(record.source_row, RowOutcome::Create)
                    }
                    RowAction::Update(id) => {
                        RowResult::planned(record.source_row, RowOutcome::Update)
                            .with_record(RecordId::Int(id))
                    }
                    RowAction::Unchanged(id) => {
                        RowResult::planned(record.source_row, RowOutcome::Unchanged)
                            .with_record(RecordId::Int(id))
                    }
                },
                Err(field_errors) => RowResult::skipped(record.source_row, field_errors),
            };
            plan.rows.push(identify(result, row, &ctx.options));
        }
        if keyless > 0 {
            plan.notices.push(keyless_notice(keyless));
        }

        Ok(plan)
    }

    /// Writes exactly the rows the dry-run counted: the same plan runs
    /// against the same lookups, and invalid rows are skipped.
    fn commit(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        ctx: &ImportContext,
    ) -> Result<Committed, CommitError> {
        if check_headers(&parsed.headers).is_some() {
            return Err(diesel::result::Error::QueryBuilderError(
                "header validation should have caught this; refusing to commit".into(),
            )
            .into());
        }
        let lookups = Lookups::load(conn)?;
        let file_keys = FileKeys::scan(parsed);

        let mut assets = Vec::new();
        let mut rows = Vec::with_capacity(parsed.rows.len());
        for record in parsed.records() {
            let (row_num, row) = (record.source_row, &record.values);
            let options = &ctx.options;
            let planned = match plan_row(row, &lookups, &file_keys, options) {
                Ok(planned) => planned,
                Err(field_errors) => {
                    rows.push(identify(
                        RowResult::skipped(row_num, field_errors),
                        row,
                        options,
                    ));
                    continue;
                }
            };
            let mark = options.decimal_mark();
            let (outcome, written) = match planned.action {
                RowAction::Create => (
                    RowOutcome::Create,
                    asset_repo::create_device(
                        conn,
                        build_new_asset(row, planned.kind, ctx.actor, mark),
                    ),
                ),
                // Routed through update_device so the asset.updated sync
                // event fires, matching the create path.
                RowAction::Update(asset_id) => (
                    RowOutcome::Update,
                    asset_repo::update_device(
                        conn,
                        asset_id,
                        build_asset_update(row, planned.kind, mark),
                    ),
                ),
                // A synced asset's row matched it, so there is nothing to write.
                RowAction::Unchanged(asset_id) => {
                    rows.push(identify(
                        RowResult::planned(row_num, RowOutcome::Unchanged)
                            .with_record(RecordId::Int(asset_id)),
                        row,
                        options,
                    ));
                    continue;
                }
            };
            let asset = written.map_err(|e| row_write_error(row_num, e))?;
            rows.push(identify(
                RowResult::planned(row_num, outcome).with_record(RecordId::Int(asset.id)),
                row,
                options,
            ));
            assets.push(asset);
        }
        Ok(Committed {
            records: ImportedRecords::Assets(assets),
            rows,
        })
    }
}

/// The row's match key and name on its result.
fn identify(
    result: RowResult,
    row: &HashMap<String, String>,
    options: &ImportOptions,
) -> RowResult {
    let key = match options.match_by {
        MatchBy::AssetTag => trimmed(row, "asset_tag"),
        MatchBy::SerialNumber => trimmed(row, "serial_number"),
        MatchBy::None => opt_string(row, "asset_tag")
            .or_else(|| opt_string(row, "serial_number"))
            .unwrap_or_default(),
    };
    result.with_key(Some(key)).with_label(trimmed(row, "name"))
}

/// A row with neither a tag nor a serial: nothing can match it later, so
/// importing the file again creates it again.
fn is_keyless(row: &HashMap<String, String>) -> bool {
    trimmed(row, "asset_tag").is_empty() && trimmed(row, "serial_number").is_empty()
}

fn keyless_notice(count: usize) -> Notice {
    let message = if count == 1 {
        "1 row has no asset tag or serial number. Importing this file again would create it again."
            .to_string()
    } else {
        format!(
            "{count} rows have no asset tag or serial number. Importing this file again would create them again."
        )
    };
    Notice {
        code: "no_key".to_string(),
        count,
        message,
    }
}

#[derive(Debug, Clone, Copy)]
enum RowAction {
    Create,
    /// Update the existing asset with this id (matched on the match key).
    Update(i32),
    /// A synced asset whose row matches what's stored: nothing to write.
    Unchanged(i32),
}

struct PlannedRow {
    action: RowAction,
    /// The kind's slug, resolved from the slug or label in the file.
    kind: String,
}

/// Workspace state every row is checked against, loaded once per run.
struct Lookups {
    /// Every asset holding each tag. Tags aren't unique, and a tag held by
    /// more than one asset can't say which to update.
    tags: HashMap<String, Vec<i32>>,
    serials: HashMap<String, i32>,
    /// Each asset with a tag or a serial, the rows an update can hit.
    existing: HashMap<i32, Asset>,
    /// Asset model names by id.
    models: HashMap<i32, String>,
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
        let rows: Vec<Asset> = assets::table
            .filter(
                assets::asset_tag
                    .is_not_null()
                    .or(assets::serial_number.is_not_null()),
            )
            .load(conn)?;
        let mut tags = HashMap::new();
        let mut serials = HashMap::new();
        let mut existing = HashMap::new();
        for asset in rows {
            if let Some(serial) = &asset.serial_number {
                serials.insert(serial.clone(), asset.id);
            }
            if let Some(tag) = &asset.asset_tag {
                tags.entry(tag.clone())
                    .or_insert_with(Vec::new)
                    .push(asset.id);
            }
            existing.insert(asset.id, asset);
        }

        let models = model_repo::list(conn)?
            .into_iter()
            .map(|m| (m.id, m.name))
            .collect();

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
            models,
            kinds,
            kind_labels,
            by_slug,
        })
    }
}

/// Tags and serials more than one row of the file holds, each with the
/// rows holding it. Every one of those rows is skipped.
struct FileKeys {
    tags: HashMap<String, Vec<usize>>,
    serials: HashMap<String, Vec<usize>>,
}

impl FileKeys {
    fn scan(parsed: &ParsedCsv) -> Self {
        let mut tags: HashMap<String, Vec<usize>> = HashMap::new();
        let mut serials: HashMap<String, Vec<usize>> = HashMap::new();
        for record in parsed.records() {
            for (column, map) in [("asset_tag", &mut tags), ("serial_number", &mut serials)] {
                let value = trimmed(&record.values, column);
                if !value.is_empty() {
                    map.entry(value).or_default().push(record.source_row);
                }
            }
        }
        tags.retain(|_, rows| rows.len() > 1);
        serials.retain(|_, rows| rows.len() > 1);
        Self { tags, serials }
    }
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
    file_keys: &FileKeys,
    options: &ImportOptions,
) -> Result<PlannedRow, Vec<(Option<String>, String)>> {
    let mut errors: Vec<(Option<String>, String)> = Vec::new();
    let mut error =
        |column: &str, message: String| errors.push((Some(column.to_string()), message));
    let mark = options.decimal_mark();

    if trimmed(row, "name").is_empty() {
        error("name", "name is required".to_string());
    }

    for (column, max) in MAX_LENGTHS {
        let len = trimmed(row, column).chars().count();
        if len > *max {
            error(column, format!("longer than {max} characters ({len})"));
        }
    }

    if let Err(message) = check_decimal(row, "quantity", mark) {
        error("quantity", message);
    }
    match check_decimal(row, "low_stock_threshold", mark) {
        Err(message) => error("low_stock_threshold", message),
        Ok(Some(v)) if v < 0 => error("low_stock_threshold", "must be 0 or more".to_string()),
        Ok(_) => {}
    }

    let tag = trimmed(row, "asset_tag");
    // Same canonical form the repository stores (trimmed, blank is none).
    let serial = trimmed(row, "serial_number");
    let tag_holders = (!tag.is_empty())
        .then(|| lookups.tags.get(&tag))
        .flatten()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let serial_holder = (!serial.is_empty())
        .then(|| lookups.serials.get(&serial).copied())
        .flatten();
    let target = match options.match_by {
        MatchBy::AssetTag => match tag_holders {
            [] => None,
            [id] => Some(*id),
            ids => {
                error("asset_tag", shared_tag_error(lookups, &tag, ids));
                None
            }
        },
        MatchBy::SerialNumber => serial_holder,
        MatchBy::None => {
            if !tag_holders.is_empty() {
                error(
                    "asset_tag",
                    format!(
                        "tag '{tag}' already exists as {}; this import creates new assets only",
                        asset_names(lookups, tag_holders)
                    ),
                );
            }
            None
        }
    };
    if let Some(rows) = file_keys.tags.get(&tag) {
        error("asset_tag", duplicate_in_file("tag", &tag, rows));
    } else if options.match_by == MatchBy::SerialNumber
        && !target.is_some_and(|id| tag_holders.contains(&id))
    {
        // Matched by serial, the row's tag must not be another asset's.
        let others: Vec<i32> = tag_holders
            .iter()
            .copied()
            .filter(|id| Some(*id) != target)
            .collect();
        if !others.is_empty() {
            error(
                "asset_tag",
                format!(
                    "tag '{tag}' is already used by {}",
                    asset_names(lookups, &others)
                ),
            );
        }
    }

    if !serial.is_empty() {
        if let Some(rows) = file_keys.serials.get(&serial) {
            error(
                "serial_number",
                duplicate_in_file("serial number", &serial, rows),
            );
        } else if let Some(holder) = serial_holder {
            if options.match_by == MatchBy::None {
                error(
                    "serial_number",
                    format!(
                        "serial number '{serial}' already exists as {}; this import creates new assets only",
                        asset_names(lookups, &[holder])
                    ),
                );
            } else if Some(holder) != target {
                error(
                    "serial_number",
                    format!("serial number '{serial}' is already used by another asset"),
                );
            }
        }
    }

    // Matched on slug or label, ignoring case. Kind attributes aren't
    // CSV columns, so the kind's required attributes aren't checked
    // here: the asset is created without them and filled in on its page.
    // A blank cell keeps an existing asset's kind and gives a new one the
    // default kind.
    let existing = target.and_then(|id| lookups.existing.get(&id));
    let kind_cell = trimmed(row, "kind");
    let kind = if kind_cell.is_empty() {
        match (existing, &options.default_kind) {
            (Some(asset), _) => Some(asset.kind.clone()),
            (None, Some(default)) => {
                let found = lookups.kinds.get(&default.to_lowercase()).cloned();
                if found.is_none() {
                    error("kind", unknown_kind_error(lookups, default));
                }
                found
            }
            (None, None) => {
                error("kind", "kind is required".to_string());
                None
            }
        }
    } else {
        let found = lookups.kinds.get(&kind_cell.to_lowercase()).cloned();
        if found.is_none() {
            error("kind", unknown_kind_error(lookups, &kind_cell));
        }
        found
    };

    // An update follows what PUT and the asset page allow. Required
    // attributes stay unchecked, as for new rows.
    let mut synced = false;
    if let (Some(new_kind), Some(asset)) = (&kind, existing) {
        if let Some(source) = &asset.external_sync_source {
            synced = true;
            if let Some(column) = synced_column_change(row, asset, new_kind, mark) {
                error(
                    column,
                    format!(
                        "{} is synced from {}, which sets its {}; unmanage it on the asset page to edit it here",
                        asset.name,
                        sync_source_name(source),
                        column.replace('_', " ")
                    ),
                );
            }
        } else if let Some(message) = kind_change_error(lookups, asset, new_kind) {
            error("kind", message);
        }
    }

    match kind {
        Some(kind) if errors.is_empty() => {
            let action = match target {
                Some(id) if synced => RowAction::Unchanged(id),
                Some(id) => RowAction::Update(id),
                None => RowAction::Create,
            };
            Ok(PlannedRow { action, kind })
        }
        _ => Err(errors),
    }
}

fn unknown_kind_error(lookups: &Lookups, kind: &str) -> String {
    format!(
        "unknown kind '{kind}'. Use one of: {}, or add it under Admin → Asset Kinds",
        lookups.kind_labels.join(", ")
    )
}

/// The names of existing assets, sorted, for an error message.
fn asset_names(lookups: &Lookups, ids: &[i32]) -> String {
    let mut names: Vec<&str> = ids
        .iter()
        .filter_map(|id| lookups.existing.get(id))
        .map(|asset| asset.name.as_str())
        .collect();
    names.sort_unstable();
    names.join(", ")
}

/// A tag more than one existing asset holds, naming them.
fn shared_tag_error(lookups: &Lookups, tag: &str, ids: &[i32]) -> String {
    format!(
        "tag '{tag}' is held by {} assets ({}); give each its own tag on the asset page, then import again",
        ids.len(),
        asset_names(lookups, ids)
    )
}

/// Display name of an `external_sync_source`, worded as on the asset page.
fn sync_source_name(source: &str) -> &'static str {
    match source {
        "entra" => "Microsoft Entra",
        "intune" => "Microsoft Intune",
        _ => "an external source",
    }
}

/// The first column, in file order, where the row would change a synced
/// asset. PUT takes none of these on a synced asset. Values compare as the
/// update would write them, so a blank cell never differs, and stored
/// values compare trimmed, so a re-imported export matches.
fn synced_column_change(
    row: &HashMap<String, String>,
    asset: &Asset,
    new_kind: &str,
    mark: DecimalMark,
) -> Option<&'static str> {
    fn stored(value: &Option<String>) -> Option<&str> {
        value.as_deref().map(str::trim).filter(|v| !v.is_empty())
    }
    // A blank cell leaves any column as it is.
    let kept = |column: &str, current: &Option<String>| {
        opt_string(row, column).is_some_and(|v| Some(v.as_str()) != stored(current))
    };
    let kept_decimal = |column: &str, current: &Option<BigDecimal>| {
        check_decimal(row, column, mark).is_ok_and(|v| v.is_some_and(|v| Some(v) != *current))
    };
    let changed: [(&'static str, bool); 10] = [
        ("name", trimmed(row, "name") != asset.name.trim()),
        ("kind", new_kind != asset.kind),
        ("serial_number", kept("serial_number", &asset.serial_number)),
        ("manufacturer", kept("manufacturer", &asset.manufacturer)),
        ("model", kept("model", &asset.model)),
        ("location", kept("location", &asset.location)),
        ("notes", kept("notes", &asset.notes)),
        ("quantity", kept_decimal("quantity", &asset.quantity)),
        ("unit", kept("unit", &asset.unit)),
        (
            "low_stock_threshold",
            kept_decimal("low_stock_threshold", &asset.low_stock_threshold),
        ),
    ];
    changed
        .into_iter()
        .find_map(|(column, changed)| changed.then_some(column))
}

/// The error for an update that moves `existing` (not synced) to
/// `new_kind`, if the asset page wouldn't allow it: its model sets its
/// kind, or it holds attributes the new kind doesn't define.
fn kind_change_error(lookups: &Lookups, existing: &Asset, new_kind: &str) -> Option<String> {
    let old_kind = &existing.kind;
    if old_kind == new_kind {
        return None;
    }
    if let Some(model_id) = existing.model_id {
        let model = lookups.models.get(&model_id).map_or_else(
            || "its model".to_string(),
            |name| format!("its model {name}"),
        );
        return Some(format!(
            "{} takes its kind from {model}; change the model on the asset page instead",
            existing.name
        ));
    }
    let attributes = &existing.attributes;
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
    let (column, message) = match &e {
        Error::DatabaseError(DatabaseErrorKind::UniqueViolation, info)
            if info.constraint_name() == Some("idx_asset_serial_unique") =>
        {
            (
                Some("serial_number".to_string()),
                "its serial number is already used by another asset".to_string(),
            )
        }
        Error::DatabaseError(
            DatabaseErrorKind::UniqueViolation
            | DatabaseErrorKind::CheckViolation
            | DatabaseErrorKind::NotNullViolation
            | DatabaseErrorKind::Unknown,
            info,
        ) => (None, info.message().to_string()),
        _ => return CommitError::Db(e),
    };
    CommitError::Row {
        row,
        column,
        message,
    }
}

fn trimmed(row: &HashMap<String, String>, key: &str) -> String {
    row.get(key).cloned().unwrap_or_default().trim().to_string()
}

/// A decimal cell: `Ok(None)` when blank, the value when it parses and
/// fits `numeric(12,3)`, otherwise the message for the admin. `mark` is
/// the decimal mark the admin chose; with `,` a `.` in the cell is an
/// error, since it may be a thousands separator.
fn check_decimal(
    row: &HashMap<String, String>,
    key: &str,
    mark: DecimalMark,
) -> Result<Option<BigDecimal>, String> {
    let raw = trimmed(row, key);
    if raw.is_empty() {
        return Ok(None);
    }
    let text = match mark {
        DecimalMark::Dot => raw.clone(),
        DecimalMark::Comma if raw.contains('.') => {
            return Err(format!(
                "'{raw}' has a '.', but the decimal mark is ','; remove thousands separators"
            ));
        }
        DecimalMark::Comma => raw.replace(',', "."),
    };
    let value = BigDecimal::from_str(&text).map_err(|_| match mark {
        DecimalMark::Dot if raw.contains(',') && !raw.contains('.') => format!(
            "'{raw}' is not a valid decimal; if ',' is the decimal mark, choose ',' at the Map step"
        ),
        _ => format!("'{raw}' is not a valid decimal"),
    })?;
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

fn opt_decimal(row: &HashMap<String, String>, key: &str, mark: DecimalMark) -> Option<BigDecimal> {
    check_decimal(row, key, mark).ok().flatten()
}

fn build_new_asset(
    row: &HashMap<String, String>,
    kind: String,
    created_by: Option<uuid::Uuid>,
    mark: DecimalMark,
) -> NewAsset {
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
        quantity: opt_decimal(row, "quantity", mark),
        unit: opt_string(row, "unit"),
        external_sync_source: None,
        low_stock_threshold: opt_decimal(row, "low_stock_threshold", mark),
        created_by,
    }
}

fn build_asset_update(
    row: &HashMap<String, String>,
    kind: String,
    mark: DecimalMark,
) -> AssetUpdate {
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
        quantity: opt_decimal(row, "quantity", mark).map(Some),
        unit: opt_string(row, "unit").map(Some),
        external_sync_source: None,
        low_stock_threshold: opt_decimal(row, "low_stock_threshold", mark).map(Some),
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
}
