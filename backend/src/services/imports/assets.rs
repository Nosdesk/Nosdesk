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
//! import never leaves two assets sharing a tag. The mode
//! (`ImportOptions::mode`) narrows which rows write: only new ones skip a
//! matched row as already there, only existing ones skip an unmatched row
//! as not found. A blank cell never clears a column on update; a
//! blank kind keeps the asset's kind, and on a new asset takes the default
//! kind when one is set.
//!
//! Kind attributes are fields of their own ([`attribute_defs`]): each key
//! a kind's schema defines, less the keys a directory sync owns. A row's
//! attribute cells are read as its kind's schema types them and merged
//! into the asset's attributes, a blank cell leaving the stored value;
//! the result is checked against the kind's schema once per row. A new
//! asset must carry its kind's required attributes. An existing one is
//! checked as `PUT /assets/{id}` checks it: when the row writes an
//! attribute or changes the kind. A value for an attribute the row's kind
//! doesn't have is left out and counted ([`RowResult::dropped`]), not an
//! error.
//!
//! An update writes only what `PUT /assets/{id}` and the asset page would.
//! An asset synced from Microsoft takes its attributes from the file and
//! no other column: the rest of its row must match what's stored. An
//! asset linked to a model keeps its kind, since the model sets it. A kind
//! change never leaves the asset holding attributes the new kind doesn't
//! define. Any other row is an error that points to the asset page.
//!
//! Status moves through the lifecycle rules
//! ([`asset_lifecycle_repo::transition`], reason Imported, naming the job),
//! after the asset's own write and, on a new asset, after its Imported
//! event: a new asset starts in service and moves from there. A blank cell
//! leaves the status. Disposed and on loan come from a disposal or a loan,
//! a disposed asset keeps its status, and one out on loan leaves it only
//! by its return; the dry run refuses each such row as the transition
//! would. A synced asset's status moves too, as on the asset page.

use bigdecimal::{BigDecimal, ToPrimitive};
use chrono::NaiveDate;
use diesel::prelude::*;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    Asset, AssetGroup, AssetKind, AssetLifecycleEvent, AssetModel, AssetStatus, AssetUpdate,
    Manufacturer, NewAsset, NewAssetGroup, NewAssetModel, NewManufacturer,
};
use crate::repository::asset_groups as group_repo;
use crate::repository::asset_kinds as kind_repo;
use crate::repository::asset_lifecycle::{self as asset_lifecycle_repo, TransitionError};
use crate::repository::asset_loans as asset_loans_repo;
use crate::repository::asset_models as model_repo;
use crate::repository::assets as asset_repo;
use crate::repository::manufacturers as manufacturer_repo;
use crate::repository::workspaces as workspace_repo;
use crate::services::assets::kinds::{validate_attributes, AttributeError};
use crate::services::assets::model_stamp::stamp_default_attributes;
use crate::services::assets::SYNC_OWNED_ATTRIBUTE_KEYS;

use super::catalog::Catalog;
use super::csv_parser::ParsedCsv;
use super::safe_cells::csv_formula_safe;
use super::types::{
    duplicate_in_file, CatalogKind, CatalogPlan, CatalogRef, CommitError, Committed, DateOrder,
    DecimalMark, DroppedCell, FieldChange, FieldGroup, FieldSpec, ImportContext, ImportMode,
    ImportOptions, ImportType, ImportedRecords, Importer, MatchBy, Notice, PlannedModel, RecordId,
    RowOutcome, RowPlan, RowResult, ValueAnswer, ValueType,
};
use super::values::{self, normalize_value, Choice, LookupKind, ValueContext};

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

/// Columns appended on export only. The importer ignores `attributes`,
/// and an unchanged `status` writes nothing, so re-importing an unmodified
/// export is a no-op upsert on the shared columns.
pub const EXPORT_EXTRA_HEADERS: &[&str] = &["status", "attributes"];

/// Every status in picker order, with its label.
const STATUSES: &[(AssetStatus, &str)] = &[
    (AssetStatus::InService, "In service"),
    (AssetStatus::InStock, "In stock"),
    (AssetStatus::OnOrder, "On order"),
    (AssetStatus::InTransit, "In transit"),
    (AssetStatus::InRepair, "In repair"),
    (AssetStatus::OnLoan, "On loan"),
    (AssetStatus::Retired, "Retired"),
    (AssetStatus::Lost, "Lost"),
    (AssetStatus::Disposed, "Disposed"),
];

/// Why disposed and on loan can't be picked for the Status column: the
/// asset page sets them, with the disposal record or the loan.
pub const STATUS_SET_AFTER_IMPORT: &str = "Set by a disposal or a loan after import";

/// Whether only a disposal or a loan sets `status`, never an import.
fn set_after_import(status: AssetStatus) -> bool {
    matches!(status, AssetStatus::Disposed | AssetStatus::OnLoan)
}

/// A status cell as the status it names, by key or label, ignoring case,
/// spacing, `_` and `-`.
fn read_status(cell: &str) -> Option<AssetStatus> {
    let wanted = values::loose(cell);
    STATUSES
        .iter()
        .find(|(status, label)| {
            values::loose(status.as_str()) == wanted || values::loose(label) == wanted
        })
        .map(|(status, _)| *status)
}

/// The template's columns before a kind's attributes, in order. An
/// attribute whose key is one of these is named `attr:<key>`.
pub const TEMPLATE_CORE_HEADERS: &[&str] = &[
    "name",
    "kind",
    "asset_tag",
    "serial_number",
    "status",
    "manufacturer",
    "model",
    "location",
    "primary_user",
    "purchase_date",
    "groups",
    "notes",
    "quantity",
    "unit",
    "low_stock_threshold",
];

/// What an attribute's field id starts with when its key is a core column,
/// as the asset list names attribute columns.
pub const ATTRIBUTE_PREFIX: &str = "attr:";

/// One attribute key a column can feed, across the kinds that define it.
#[derive(Debug, Clone)]
pub struct AttributeDef {
    pub key: String,
    /// Field id: the key, or `attr:<key>` when the key is a core column.
    pub id: String,
    /// The title the first kind defining it gives it, else the key in words.
    pub title: String,
    /// Each kind that defines it, in kind order: slug, label and the
    /// property's schema.
    pub kinds: Vec<(String, String, Value)>,
    /// Kinds whose schema requires it.
    pub required_for: Vec<String>,
}

impl AttributeDef {
    fn prop(&self, kind: &str) -> Option<&Value> {
        self.kinds
            .iter()
            .find(|(slug, _, _)| slug == kind)
            .map(|(_, _, prop)| prop)
    }

    /// The field a column maps to: typed as the first kind defining it
    /// types it, with every kind's enum options.
    fn field_spec(&self) -> FieldSpec {
        let first = self.kinds.first().map(|(_, _, prop)| prop);
        let mut options: Vec<String> = Vec::new();
        for (_, _, prop) in &self.kinds {
            for option in enum_options(prop) {
                if !options.contains(&option) {
                    options.push(option);
                }
            }
        }
        let value_type = first.map_or(ValueType::Text, value_type_of);
        let mut spec = attribute_spec(&self.id, &self.title, value_type);
        spec.kinds = Some(self.kinds.iter().map(|(slug, _, _)| slug.clone()).collect());
        if value_type == ValueType::Enum {
            spec.enum_values = Some(options);
        }
        if !self.required_for.is_empty() {
            spec.required_for = Some(self.required_for.clone());
        }
        spec
    }
}

/// An attribute field, never required at the Map step.
fn attribute_spec(id: &str, title: &str, value_type: ValueType) -> FieldSpec {
    let mut spec = FieldSpec::fixed(
        ImportType::Assets,
        id,
        title,
        FieldGroup::Attribute,
        value_type,
        false,
    );
    spec.label_key = None;
    if value_type == ValueType::Date {
        spec.format = Some("YYYY-MM-DD".to_string());
    }
    spec
}

/// A property's enum options that are text.
fn enum_options(prop: &Value) -> Vec<String> {
    prop.get("enum")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// How a property's cells read.
fn value_type_of(prop: &Value) -> ValueType {
    match prop.get("type").and_then(Value::as_str) {
        Some("boolean") => return ValueType::Boolean,
        Some("number" | "integer") => return ValueType::Number,
        Some("array") => return ValueType::Array,
        _ => {}
    }
    if prop.get("enum").and_then(Value::as_array).is_some() {
        return ValueType::Enum;
    }
    match prop.get("format").and_then(Value::as_str) {
        Some("date") => ValueType::Date,
        Some("date-time") => ValueType::DateTime,
        Some("user-uuid") => ValueType::User,
        Some("asset-ref") => ValueType::AssetRef,
        Some("email") => ValueType::Email,
        _ => ValueType::Text,
    }
}

/// `warranty_end` as "Warranty end".
fn humanize(key: &str) -> String {
    let words = key.replace('_', " ");
    let words = words.trim();
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A property's title, else its key in words.
fn title_of(key: &str, prop: &Value) -> String {
    prop.get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map_or_else(|| humanize(key), String::from)
}

/// Whether a property is fed by a directory sync rather than typed.
fn is_synced(key: &str, prop: &Value) -> bool {
    SYNC_OWNED_ATTRIBUTE_KEYS.contains(&key)
        || prop.get("synced").and_then(Value::as_bool) == Some(true)
}

/// Every attribute a column can feed, from the kinds in kind order, sorted
/// by title. Keys a directory sync owns, and properties marked `synced`,
/// are left out.
pub fn attribute_defs(kinds: &[AssetKind]) -> Vec<AttributeDef> {
    let mut defs: Vec<AttributeDef> = Vec::new();
    for kind in kinds {
        let Some(properties) = kind
            .attribute_schema
            .get("properties")
            .and_then(Value::as_object)
        else {
            continue;
        };
        let required = required_keys(&kind.attribute_schema);
        for (key, prop) in properties {
            if is_synced(key, prop) {
                continue;
            }
            let at = match defs.iter().position(|d| &d.key == key) {
                Some(at) => at,
                None => {
                    let id = if TEMPLATE_CORE_HEADERS.contains(&key.as_str()) {
                        format!("{ATTRIBUTE_PREFIX}{key}")
                    } else {
                        key.clone()
                    };
                    defs.push(AttributeDef {
                        key: key.clone(),
                        id,
                        title: title_of(key, prop),
                        kinds: Vec::new(),
                        required_for: Vec::new(),
                    });
                    defs.len() - 1
                }
            };
            let def = &mut defs[at];
            def.kinds
                .push((kind.slug.clone(), kind.label.clone(), prop.clone()));
            if required.contains(key) {
                def.required_for.push(kind.slug.clone());
            }
        }
    }
    defs.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.key.cmp(&b.key))
    });
    defs
}

/// The keys a kind's schema requires.
fn required_keys(schema: &Value) -> HashSet<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|keys| {
            keys.iter()
                .filter_map(|k| k.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// The template's header row for `kind` (a slug, or `None` for core
/// columns only): the core columns, then the kind's attributes sorted by
/// their title. `None` when no kind has that slug.
pub fn template_headers(
    conn: &mut DbConnection,
    kind: Option<&str>,
) -> Result<Option<Vec<String>>, diesel::result::Error> {
    let mut headers: Vec<String> = TEMPLATE_CORE_HEADERS
        .iter()
        .map(ToString::to_string)
        .collect();
    let Some(kind) = kind else {
        return Ok(Some(headers));
    };
    let kinds = kind_repo::list_kinds(conn)?;
    if !kinds.iter().any(|k| k.slug == kind) {
        return Ok(None);
    }
    let mut own: Vec<(String, String)> = attribute_defs(&kinds)
        .into_iter()
        .filter_map(|def| {
            let title = title_of(&def.key, def.prop(kind)?);
            Some((title, def.id))
        })
        .collect();
    own.sort_by(|a, b| {
        a.0.to_lowercase()
            .cmp(&b.0.to_lowercase())
            .then_with(|| a.1.cmp(&b.1))
    });
    headers.extend(own.into_iter().map(|(_, id)| id));
    Ok(Some(headers))
}

/// The template as CSV bytes: a BOM so Excel reads it as UTF-8, then the
/// header row, each cell guarded against formulas.
pub fn template_csv(headers: &[String]) -> Result<Vec<u8>, csv::Error> {
    let mut buf = "\u{FEFF}".as_bytes().to_vec();
    {
        let mut wtr = csv::Writer::from_writer(&mut buf);
        wtr.write_record(headers.iter().map(|h| csv_formula_safe(h.clone())))?;
        wtr.flush()?;
    }
    Ok(buf)
}

pub struct AssetImporter;

impl Importer for AssetImporter {
    fn template_headers(&self) -> &'static [&'static str] {
        TEMPLATE_CORE_HEADERS
    }

    fn fields(&self, conn: &mut DbConnection) -> Result<Vec<FieldSpec>, diesel::result::Error> {
        use FieldGroup::{Core, Links};
        use ValueType::{Date, Enum, Number, Text, User};
        let t = ImportType::Assets;
        let registry = kind_repo::list_kinds(conn)?;
        let kinds = registry.iter().map(|k| k.slug.clone()).collect();
        use ValueType::Array;
        let mut fields = vec![
            FieldSpec::fixed(t, "name", "Name", Core, Text, true),
            FieldSpec::fixed(t, "kind", "Kind", Core, Enum, true).with_enum(kinds),
            FieldSpec::fixed(t, "asset_tag", "Asset tag", Core, Text, false),
            FieldSpec::fixed(t, "serial_number", "Serial number", Core, Text, false),
            FieldSpec::fixed(t, "status", "Status", Core, Enum, false).with_enum(
                STATUSES
                    .iter()
                    .filter(|(status, _)| !set_after_import(*status))
                    .map(|(status, _)| status.as_str().to_string())
                    .collect(),
            ),
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
            FieldSpec::fixed(t, "purchase_date", "Purchase date", Core, Date, false)
                .with_format("YYYY-MM-DD"),
            FieldSpec::fixed(t, "primary_user", "Primary user", Core, User, false),
            FieldSpec::fixed(t, "manufacturer", "Manufacturer", Links, Text, false),
            FieldSpec::fixed(t, "model", "Model", Links, Text, false),
            FieldSpec::fixed(t, "groups", "Groups", Links, Array, false),
        ];
        fields.extend(
            attribute_defs(&registry)
                .iter()
                .map(AttributeDef::field_spec),
        );
        Ok(fields)
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
            ("purchased", "purchase_date"),
            ("dateofpurchase", "purchase_date"),
            ("datepurchased", "purchase_date"),
            ("assignedto", "primary_user"),
            ("assigneduser", "primary_user"),
            ("user", "primary_user"),
            ("usedby", "primary_user"),
            ("group", "groups"),
            ("assetgroup", "groups"),
            ("assetgroups", "groups"),
            ("tags", "groups"),
            ("state", "status"),
            ("assetstatus", "status"),
            ("lifecyclestatus", "status"),
        ]
    }

    fn value_choices(
        &self,
        conn: &mut DbConnection,
        field: &FieldSpec,
    ) -> Result<Option<Vec<Choice>>, diesel::result::Error> {
        if field.id == "status" {
            return Ok(Some(
                STATUSES
                    .iter()
                    .map(|(status, label)| {
                        let mut choice = Choice::new(status.as_str(), *label);
                        if set_after_import(*status) {
                            choice.disabled = Some(STATUS_SET_AFTER_IMPORT.to_string());
                        }
                        choice
                    })
                    .collect(),
            ));
        }
        if field.id != "kind" {
            return Ok(values::enum_choices(field));
        }
        Ok(Some(
            kind_repo::list_kinds(conn)?
                .into_iter()
                .map(|k| Choice::new(k.slug, k.label))
                .collect(),
        ))
    }

    fn value_synonyms(&self, field: &str) -> &'static [(&'static str, &'static str)] {
        match field {
            "kind" => &[
                ("cam", "camera"),
                ("notebook", "laptop"),
                ("macbook", "laptop"),
                ("pc", "desktop"),
                ("workstation", "desktop"),
                ("display", "monitor"),
                ("screen", "monitor"),
                ("mobile", "phone"),
                ("mobile phone", "phone"),
                ("cell phone", "phone"),
                ("smartphone", "phone"),
                ("switch", "network_device"),
                ("router", "network_device"),
                ("access point", "network_device"),
                ("car", "vehicle"),
                ("van", "vehicle"),
                ("truck", "vehicle"),
                ("ute", "vehicle"),
                ("licence", "license"),
                ("software", "license"),
            ],
            "status" => &[
                ("in use", "in_service"),
                ("active", "in_service"),
                ("deployed", "in_service"),
                ("assigned", "in_service"),
                ("live", "in_service"),
                ("spare", "in_stock"),
                ("available", "in_stock"),
                ("in storage", "in_stock"),
                ("stored", "in_stock"),
                ("storage", "in_stock"),
                ("unassigned", "in_stock"),
                ("broken", "in_repair"),
                ("faulty", "in_repair"),
                ("damaged", "in_repair"),
                ("repair", "in_repair"),
                ("needs repair", "in_repair"),
                ("being repaired", "in_repair"),
                ("ordered", "on_order"),
                ("on backorder", "on_order"),
                ("shipping", "in_transit"),
                ("shipped", "in_transit"),
                ("loaned", "on_loan"),
                ("lent", "on_loan"),
                ("borrowed", "on_loan"),
                ("decommissioned", "retired"),
                ("end of life", "retired"),
                ("eol", "retired"),
                ("inactive", "retired"),
                ("missing", "lost"),
                ("stolen", "lost"),
                ("scrapped", "disposed"),
                ("recycled", "disposed"),
                ("written off", "disposed"),
            ],
            _ => &[],
        }
    }

    /// Attributes are answered per kind: each kind's options, typed as that
    /// kind's schema types the key.
    fn value_contexts(
        &self,
        conn: &mut DbConnection,
        field: &FieldSpec,
    ) -> Result<Option<Vec<ValueContext>>, diesel::result::Error> {
        if field.group != FieldGroup::Attribute {
            return Ok(None);
        }
        let kinds = kind_repo::list_kinds(conn)?;
        let Some(def) = attribute_defs(&kinds)
            .into_iter()
            .find(|d| d.id == field.id)
        else {
            return Ok(Some(Vec::new()));
        };
        Ok(Some(
            def.kinds
                .iter()
                .map(|(slug, label, prop)| {
                    let mut spec = attribute_spec(&def.id, &field.label, value_type_of(prop));
                    let options = enum_options(prop);
                    if spec.value_type == ValueType::Enum {
                        spec.enum_values = Some(options);
                    }
                    ValueContext {
                        key: slug.clone(),
                        label: label.clone(),
                        spec,
                    }
                })
                .collect(),
        ))
    }

    /// Attributes are answered per kind, models per manufacturer.
    fn context_family(&self, field: &FieldSpec) -> Option<&'static str> {
        if field.group == FieldGroup::Attribute {
            Some(KIND_FAMILY)
        } else if field.id == "model" {
            Some(MANUFACTURER_FAMILY)
        } else {
            None
        }
    }

    /// A model's kind can be a row's kind, so models settle first.
    fn context_families(&self) -> &'static [&'static str] {
        &[MANUFACTURER_FAMILY, KIND_FAMILY]
    }

    fn lookups(
        &self,
        conn: &mut DbConnection,
        field: &FieldSpec,
        contexts: &[String],
    ) -> Result<Option<Vec<values::LookupContext>>, diesel::result::Error> {
        let Some(kind) = lookup_kind(&field.id) else {
            return Ok(None);
        };
        Ok(Some(Catalog::load(conn)?.lookups(kind, contexts)))
    }

    /// Each row's kind, as [`plan_row`] reads it, or the manufacturer its
    /// model is matched under.
    fn row_contexts(
        &self,
        conn: &mut DbConnection,
        grid: &ParsedCsv,
        options: &ImportOptions,
        family: &str,
    ) -> Result<HashMap<usize, String>, diesel::result::Error> {
        let lookups = Lookups::load(conn)?;
        let mut out = HashMap::new();
        for record in grid.records() {
            let row = with_constants(record.values, &lookups, options);
            let context = match family {
                MANUFACTURER_FAMILY => Some(pick_maker(&row, &lookups, options).context(&lookups)),
                KIND_FAMILY => row_kind(&row, &lookups, options),
                _ => None,
            };
            if let Some(context) = context {
                out.insert(record.source_row, context);
            }
        }
        Ok(out)
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
        let planned = plan_all(parsed, &lookups, &ctx.options);
        let mut plan = RowPlan::default();
        let mut keyless = 0;

        for entry in planned.entries {
            let row = &entry.row;
            let result = match entry.planned {
                Ok(planned) => {
                    let mut result = match planned.action {
                        RowAction::Create => {
                            if is_keyless(row) {
                                keyless += 1;
                            }
                            RowResult::planned(entry.source_row, RowOutcome::Create)
                        }
                        RowAction::Update(id) | RowAction::SyncedUpdate(id) => {
                            RowResult::planned(entry.source_row, RowOutcome::Update)
                                .with_record(RecordId::Int(id))
                        }
                        RowAction::Unchanged(id) => {
                            RowResult::planned(entry.source_row, RowOutcome::Unchanged)
                                .with_record(RecordId::Int(id))
                        }
                    };
                    result.dropped = planned.dropped;
                    // The status move the commit makes, so a commit can
                    // tell when the asset's status moved since.
                    if let Some(to) = planned.status_to {
                        let before = match planned.action {
                            RowAction::Update(id) | RowAction::SyncedUpdate(id) => {
                                lookups.existing.get(&id).map(|a| a.status.clone())
                            }
                            _ => None,
                        };
                        result.fields_written.insert(
                            "status".to_string(),
                            FieldChange {
                                before,
                                after: Some(to.as_str().to_string()),
                            },
                        );
                    }
                    result
                }
                Err(field_errors) => RowResult::skipped(entry.source_row, field_errors),
            };
            plan.rows.push(identify(result, row, &ctx.options));
        }
        if keyless > 0 {
            plan.notices.push(keyless_notice(keyless));
        }
        if planned.locations_snapped > 0 {
            plan.notices
                .push(location_notice(planned.locations_snapped));
        }
        plan.catalog = planned.catalog;

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
        let planned = plan_all(parsed, &lookups, &ctx.options);
        let names = planned.names;
        let options = &ctx.options;
        let mark = options.decimal_mark();
        let mut made = Made::default();

        let mut assets = Vec::new();
        let mut rows = Vec::with_capacity(parsed.rows.len());
        for entry in planned.entries {
            let row_num = entry.source_row;
            let row = &entry.row;
            let planned = match entry.planned {
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
            // A synced asset whose row matches what's stored: nothing to
            // write.
            if let RowAction::Unchanged(asset_id) = planned.action {
                let mut result = RowResult::planned(row_num, RowOutcome::Unchanged)
                    .with_record(RecordId::Int(asset_id));
                result.dropped = planned.dropped;
                rows.push(identify(result, row, options));
                continue;
            }

            let mut created = Vec::new();
            let texts = made
                .settle(conn, &lookups, &names, &planned, ctx.actor, &mut created)
                .map_err(|e| row_write_error(row_num, e))?;
            let (outcome, written) = match planned.action {
                RowAction::Create => (
                    RowOutcome::Create,
                    asset_repo::create_device_linked(
                        conn,
                        build_new_asset(row, &planned, &texts, ctx.actor, mark),
                        texts.link,
                    ),
                ),
                // Routed through update_device so the asset.updated sync
                // event fires, matching the create path.
                RowAction::Update(asset_id) => (
                    RowOutcome::Update,
                    asset_repo::update_device(
                        conn,
                        asset_id,
                        build_asset_update(row, &planned, &texts, mark),
                    ),
                ),
                // A synced asset takes its attributes and groups and
                // nothing else, as PUT re-scopes its update; its status
                // moves as the asset page moves it.
                RowAction::SyncedUpdate(asset_id) if planned.synced_writes => (
                    RowOutcome::Update,
                    asset_repo::update_device(
                        conn,
                        asset_id,
                        AssetUpdate {
                            attributes: planned.attributes.clone(),
                            updated_at: Some(chrono::Utc::now().naive_utc()),
                            ..Default::default()
                        },
                    ),
                ),
                RowAction::SyncedUpdate(asset_id) => (
                    RowOutcome::Update,
                    asset_repo::get_device_by_id(conn, asset_id),
                ),
                RowAction::Unchanged(_) => unreachable!("handled above"),
            };
            let mut asset = written.map_err(|e| row_write_error(row_num, e))?;
            let before = match planned.action {
                RowAction::Update(id) | RowAction::SyncedUpdate(id) => lookups.existing.get(&id),
                _ => None,
            };
            let mut result =
                RowResult::planned(row_num, outcome).with_record(RecordId::Int(asset.id));
            result.dropped = planned.dropped;
            result.catalog_created = created;
            // Additive: the groups the asset is in, and the row's.
            if !texts.groups.is_empty() {
                let current: Vec<i32> = lookups
                    .asset_groups
                    .get(&asset.id)
                    .cloned()
                    .unwrap_or_default();
                let added: Vec<i32> = texts
                    .groups
                    .iter()
                    .copied()
                    .filter(|id| !current.contains(id))
                    .collect();
                if !added.is_empty() {
                    let mut desired = current.clone();
                    desired.extend(&added);
                    let set = group_repo::set_groups_for_asset(conn, asset.id, &desired, ctx.actor)
                        .map_err(|e| row_write_error(row_num, e))?;
                    result.groups_added =
                        set.into_iter().filter(|id| !current.contains(id)).collect();
                }
            }
            // A new asset's first event, then the status move: the last
            // event the import writes is the one the row keeps.
            if outcome == RowOutcome::Create {
                let event =
                    asset_lifecycle_repo::record_imported(conn, &asset, ctx.actor, ctx.job_id)?;
                result.transition_id = Some(event.id);
            }
            if let Some(to) = planned.status_to {
                let (moved, event) = move_status(conn, asset, to, ctx, row_num)?;
                asset = moved;
                if let Some(event) = event {
                    result.transition_id = Some(event.id);
                }
            }
            result.fields_written = fields_written(before, &asset, &lookups);
            // Read back from the row's last write to the asset.
            result.observed_updated_at = Some(observed_timestamp(asset.updated_at));
            rows.push(identify(result, row, options));
            assets.push(asset);
        }
        Ok(Committed {
            records: ImportedRecords::Assets(assets),
            rows,
        })
    }
}

/// The row with the job's constants as the record takes them: a row that
/// matches an existing asset drops the ones only new assets take.
fn with_constants(
    mut row: HashMap<String, String>,
    lookups: &Lookups,
    options: &ImportOptions,
) -> HashMap<String, String> {
    if !options.constants.is_empty() && matches_existing(&row, lookups, options) {
        options.drop_new_only_constants(&mut row);
    }
    row
}

/// Whether the row's key names an existing asset (one or several).
fn matches_existing(
    row: &HashMap<String, String>,
    lookups: &Lookups,
    options: &ImportOptions,
) -> bool {
    match options.key() {
        MatchBy::AssetTag | MatchBy::None => {
            let tag = trimmed(row, "asset_tag");
            !tag.is_empty() && lookups.tags.contains_key(&tag)
        }
        MatchBy::SerialNumber => {
            let serial = trimmed(row, "serial_number");
            !serial.is_empty() && lookups.serials.contains_key(&serial)
        }
    }
}

/// The one existing asset the row's key names, if it names exactly one.
fn matched_asset<'a>(
    row: &HashMap<String, String>,
    lookups: &'a Lookups,
    options: &ImportOptions,
) -> Option<&'a Asset> {
    let id = match options.key() {
        MatchBy::AssetTag | MatchBy::None => match lookups.tags.get(&trimmed(row, "asset_tag")) {
            Some(ids) if ids.len() == 1 => Some(ids[0]),
            _ => None,
        },
        MatchBy::SerialNumber => lookups.serials.get(&trimmed(row, "serial_number")).copied(),
    }?;
    lookups.existing.get(&id)
}

/// The row's kind as a slug: its kind cell, or for a blank cell the kind of
/// the model it links, else the matched asset's kind, else the default
/// kind. `None` when the cell names no kind.
fn row_kind(
    row: &HashMap<String, String>,
    lookups: &Lookups,
    options: &ImportOptions,
) -> Option<String> {
    let cell = trimmed(row, "kind");
    if !cell.is_empty() {
        return lookups.kinds.get(&cell.to_lowercase()).cloned();
    }
    let maker = pick_maker(row, lookups, options);
    let context = maker.context(lookups);
    if let ModelPick::Link(id) = pick_model(row, lookups, options, &context).0 {
        if let Some(model) = lookups.catalog.model(id) {
            return Some(model.kind.clone());
        }
    }
    if let Some(asset) = matched_asset(row, lookups, options) {
        return Some(asset.kind.clone());
    }
    let default = options.default_kind.as_deref()?;
    lookups.kinds.get(&default.to_lowercase()).cloned()
}

/// A value as `fields_written` keeps it: text, blank as `None`.
fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(value_text)
                .collect::<Vec<_>>()
                .join("; "),
        ),
        other => Some(other.to_string()),
    }
}

/// An asset's importable fields as text by field id, blank as `None`. The
/// primary user reads as their email.
fn field_values(asset: &Asset, lookups: &Lookups) -> BTreeMap<String, Option<String>> {
    let text = |v: &Option<String>| v.clone().filter(|v| !v.is_empty());
    let decimal = |v: &Option<BigDecimal>| v.as_ref().map(ToString::to_string);
    let mut out: BTreeMap<String, Option<String>> = [
        ("name", Some(asset.name.clone())),
        ("kind", Some(asset.kind.clone())),
        ("status", Some(asset.status.clone())),
        ("asset_tag", text(&asset.asset_tag)),
        ("serial_number", text(&asset.serial_number)),
        ("manufacturer", text(&asset.manufacturer)),
        ("model", text(&asset.model)),
        ("location", text(&asset.location)),
        ("notes", text(&asset.notes)),
        ("quantity", decimal(&asset.quantity)),
        ("unit", text(&asset.unit)),
        ("low_stock_threshold", decimal(&asset.low_stock_threshold)),
        (
            "purchase_date",
            asset
                .purchase_date
                .map(|d| d.format("%Y-%m-%d").to_string()),
        ),
        (
            "primary_user",
            asset.primary_user_uuid.map(|u| {
                lookups
                    .member_emails
                    .get(&u)
                    .cloned()
                    .unwrap_or_else(|| u.to_string())
            }),
        ),
    ]
    .into_iter()
    .map(|(field, value)| (field.to_string(), value))
    .collect();
    if let Some(attributes) = asset.attributes.as_object() {
        for (key, value) in attributes {
            out.insert(lookups.attribute_id(key), value_text(value));
        }
    }
    out
}

/// What a write changed on an asset, field by field: every field it holds
/// for a new asset (`before` is `None`), only the changed ones for an
/// existing one.
fn fields_written(
    before: Option<&Asset>,
    after: &Asset,
    lookups: &Lookups,
) -> BTreeMap<String, FieldChange> {
    let old = before.map(|a| field_values(a, lookups)).unwrap_or_default();
    let new = field_values(after, lookups);
    let fields: HashSet<&String> = old.keys().chain(new.keys()).collect();
    fields
        .into_iter()
        .filter_map(|field| {
            let was = old.get(field).cloned().flatten();
            let now = new.get(field).cloned().flatten();
            (was != now).then(|| {
                (
                    field.clone(),
                    FieldChange {
                        before: was,
                        after: now,
                    },
                )
            })
        })
        .collect()
}

/// `assets.updated_at` as row results keep it: microseconds, as Postgres
/// stores it, so it compares equal to the column.
fn observed_timestamp(at: chrono::NaiveDateTime) -> String {
    at.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}

/// The row's match key and name on its result.
fn identify(
    result: RowResult,
    row: &HashMap<String, String>,
    options: &ImportOptions,
) -> RowResult {
    let key = match options.key() {
        MatchBy::AssetTag | MatchBy::None => trimmed(row, "asset_tag"),
        MatchBy::SerialNumber => trimmed(row, "serial_number"),
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
    /// A synced asset whose row changes only its attributes: write those.
    SyncedUpdate(i32),
    /// A synced asset whose row matches what's stored: nothing to write.
    Unchanged(i32),
}

struct PlannedRow {
    action: RowAction,
    /// The kind's slug, resolved from the slug or label in the file.
    kind: String,
    /// The asset's attributes with the row's merged in, when the row
    /// writes any.
    attributes: Option<Value>,
    purchase_date: Option<NaiveDate>,
    primary_user: Option<Uuid>,
    /// Values the row's kind has no attribute for, and groups left off.
    dropped: Vec<DroppedCell>,
    maker: MakerPick,
    model: ModelPick,
    /// The row links the asset to a model it isn't linked to: a new
    /// asset's, or another one.
    model_changes: bool,
    /// Groups the row lists, linked or created.
    groups: Vec<GroupPick>,
    /// The location as written: the cell, or the existing spelling it
    /// differs from only by case or spacing.
    location: Option<String>,
    location_snapped: bool,
    /// The status the commit moves the asset to through the lifecycle
    /// rules, when the row's status differs from the asset's (a new asset
    /// starts in service).
    status_to: Option<AssetStatus>,
    /// A synced asset's row writes its attributes or groups, not only its
    /// status.
    synced_writes: bool,
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
    /// The attributes columns can feed.
    attributes: Vec<AttributeDef>,
    /// Members by lowercased primary email.
    members: HashMap<String, Uuid>,
    /// Members' primary emails by user.
    member_emails: HashMap<Uuid, String>,
    /// Manufacturers, models, groups and locations rows link to.
    catalog: Catalog,
    /// Each asset's groups that aren't archived.
    asset_groups: HashMap<i32, Vec<i32>>,
    /// Assets with a loan out now.
    on_loan: HashSet<i32>,
}

struct KindInfo {
    label: String,
    /// Attribute keys the kind's schema defines.
    properties: HashSet<String>,
    schema: Value,
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

        let catalog = Catalog::load(conn)?;
        let models = catalog
            .models
            .iter()
            .map(|m| (m.id, m.name.clone()))
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
        let attributes = attribute_defs(&registry);
        let by_slug = registry
            .into_iter()
            .map(|k| {
                let properties = k
                    .attribute_schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .map(|props| props.keys().cloned().collect())
                    .unwrap_or_default();
                let info = KindInfo {
                    label: k.label,
                    properties,
                    schema: k.attribute_schema,
                };
                (k.slug, info)
            })
            .collect();

        let mut members = HashMap::new();
        let mut member_emails = HashMap::new();
        for (user, email) in workspace_repo::member_primary_emails(conn)? {
            members.insert(email.to_lowercase(), user);
            member_emails.insert(user, email);
        }

        Ok(Self {
            tags,
            serials,
            existing,
            models,
            kinds,
            kind_labels,
            by_slug,
            attributes,
            members,
            member_emails,
            catalog,
            asset_groups: group_repo::active_group_ids_by_asset(conn)?,
            on_loan: asset_loans_repo::assets_on_active_loan(conn)?
                .into_iter()
                .collect(),
        })
    }

    /// The field id of an attribute key.
    fn attribute_id(&self, key: &str) -> String {
        self.attributes
            .iter()
            .find(|d| d.key == key)
            .map_or_else(|| key.to_string(), |d| d.id.clone())
    }

    fn kind_label(&self, slug: &str) -> String {
        self.by_slug
            .get(slug)
            .map_or_else(|| slug.to_string(), |k| k.label.clone())
    }

    /// The member a cell names by primary email (any case) or user id.
    fn member(&self, cell: &str) -> Option<Uuid> {
        if cell.contains('@') {
            return self.members.get(&cell.to_lowercase()).copied();
        }
        let id = Uuid::parse_str(cell).ok()?;
        self.member_emails.contains_key(&id).then_some(id)
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

/// Why a row is skipped under [`ImportMode::CreateOnly`]: its key names an
/// asset that already exists.
pub const ALREADY_EXISTS: &str = "already exists";

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

    let purchase_date = match opt_string(row, "purchase_date") {
        None => None,
        Some(raw) => match values::parse_date(&raw, options.date_order) {
            Some(date) => Some(date),
            None => {
                error("purchase_date", invalid_date(&raw, options.date_order));
                None
            }
        },
    };
    let primary_user = match opt_string(row, "primary_user") {
        None => None,
        Some(raw) => match lookups.member(&raw) {
            Some(user) => Some(user),
            None => {
                error("primary_user", not_a_member(&raw));
                None
            }
        },
    };

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
    let by_serial = options.key() == MatchBy::SerialNumber;
    let (key_column, key_value) = if by_serial {
        ("serial_number", &serial)
    } else {
        ("asset_tag", &tag)
    };
    // The mode's own verdicts come first and alone: a row the mode leaves
    // out is skipped for that one reason, whatever else it holds.
    let matched = if by_serial {
        serial_holder.is_some()
    } else {
        !tag_holders.is_empty()
    };
    match options.row_mode() {
        ImportMode::CreateOnly if matched => {
            return Err(vec![(
                Some(key_column.to_string()),
                ALREADY_EXISTS.to_string(),
            )]);
        }
        ImportMode::UpdateOnly if !matched => {
            let message = match (by_serial, key_value.is_empty()) {
                (false, false) => "not found: no asset has this tag",
                (false, true) => "not found: no asset tag to match an asset by",
                (true, false) => "not found: no asset has this serial number",
                (true, true) => "not found: no serial number to match an asset by",
            };
            return Err(vec![(Some(key_column.to_string()), message.to_string())]);
        }
        _ => {}
    }
    let target = if by_serial {
        serial_holder
    } else {
        match tag_holders {
            [] => None,
            [id] => Some(*id),
            ids => {
                error("asset_tag", shared_tag_error(lookups, &tag, ids));
                None
            }
        }
    };
    if let Some(rows) = file_keys.tags.get(&tag) {
        error("asset_tag", duplicate_in_file("tag", &tag, rows));
    } else if by_serial && !target.is_some_and(|id| tag_holders.contains(&id)) {
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
            if Some(holder) != target {
                error(
                    "serial_number",
                    format!("serial number '{serial}' is already used by another asset"),
                );
            }
        }
    }

    // The catalog: the manufacturer, the model matched under it, and the
    // location's spelling.
    let existing = target.and_then(|id| lookups.existing.get(&id));
    let status_to = match plan_status(row, existing, lookups) {
        Ok(to) => to,
        Err(message) => {
            error("status", message);
            None
        }
    };
    let maker = pick_maker(row, lookups, options);
    if let MakerPick::Ambiguous(message) = &maker {
        error("manufacturer", message.clone());
    }
    let context = maker.context(lookups);
    let (model, model_error) = pick_model(row, lookups, options, &context);
    if let Some(message) = model_error {
        error("model", message);
    }
    let linked = match &model {
        ModelPick::Link(id) => lookups.catalog.model(*id),
        _ => None,
    };
    let model_changes = match &model {
        ModelPick::Link(id) => existing.is_none_or(|a| a.model_id != Some(*id)),
        ModelPick::New { .. } => true,
        _ => false,
    };
    let location_cell = trimmed(row, "location");
    let (location, location_snapped) = if location_cell.is_empty() {
        (None, false)
    } else {
        match lookups.catalog.snap_location(&location_cell) {
            Some(spelling) => (Some(spelling.to_string()), true),
            None => (Some(location_cell), false),
        }
    };

    // Matched on slug or label, ignoring case. A blank cell takes the
    // linked model's kind, else keeps an existing asset's kind and gives a
    // new one the default kind. A kind cell must agree with the model.
    let kind_cell = trimmed(row, "kind");
    let kind = if kind_cell.is_empty() {
        match (linked, existing, &options.default_kind) {
            (Some(model), _, _) => Some(model.kind.clone()),
            (None, Some(asset), _) => Some(asset.kind.clone()),
            (None, None, Some(default)) => {
                let found = lookups.kinds.get(&default.to_lowercase()).cloned();
                if found.is_none() {
                    error("kind", unknown_kind_error(lookups, default));
                }
                found
            }
            (None, None, None) => {
                error("kind", "kind is required".to_string());
                None
            }
        }
    } else {
        let found = lookups.kinds.get(&kind_cell.to_lowercase()).cloned();
        match (&found, linked) {
            (None, _) => error("kind", unknown_kind_error(lookups, &kind_cell)),
            (Some(found), Some(model)) if *found != model.kind => error(
                "kind",
                format!(
                    "the model {} is a {}, not a {}",
                    model.name,
                    lookups.kind_label(&model.kind),
                    lookups.kind_label(found)
                ),
            ),
            _ => {}
        }
        found
    };

    // A model the import creates needs a manufacturer, and a kind whose
    // new models need no attribute values: it is created without any.
    if let ModelPick::New { text, .. } = &model {
        if !matches!(maker, MakerPick::Link(_) | MakerPick::New { .. }) {
            error("model", format!("'{text}' needs a manufacturer"));
        } else if let Some(info) = kind.as_ref().and_then(|k| lookups.by_slug.get(k)) {
            let mut needed: Vec<String> = required_keys(&info.schema)
                .into_iter()
                .map(|key| {
                    let prop = info.schema.pointer(&format!("/properties/{key}"));
                    prop.map_or_else(|| humanize(&key), |prop| title_of(&key, prop))
                })
                .collect();
            if !needed.is_empty() {
                needed.sort_unstable();
                error(
                    "model",
                    format!(
                        "'{text}' can't be created here: new {} models need {}; add it under Asset models first",
                        info.label,
                        needed.join(", ")
                    ),
                );
            }
        }
    }

    let (groups, group_drops, group_errors) =
        pick_groups(row, lookups, options, kind.as_deref().unwrap_or(""));
    for message in group_errors {
        error("groups", message);
    }
    let current_groups: &[i32] = existing
        .and_then(|a| lookups.asset_groups.get(&a.id))
        .map(Vec::as_slice)
        .unwrap_or_default();
    let groups_change = groups.iter().any(|g| match g {
        GroupPick::Link(id) => !current_groups.contains(id),
        GroupPick::New { .. } => true,
    });

    // An update follows what PUT and the asset page allow.
    let mut synced = false;
    let mut kind_ok = true;
    if let (Some(new_kind), Some(asset)) = (&kind, existing) {
        if let Some(source) = &asset.external_sync_source {
            synced = true;
            let texts = PlannedText {
                maker: maker_text(&maker, &model, lookups),
                model: model_text(&model, lookups),
                location: location.clone(),
            };
            let column = if model_changes {
                Some("model")
            } else {
                synced_column_change(
                    row,
                    asset,
                    new_kind,
                    mark,
                    purchase_date,
                    primary_user,
                    &texts,
                )
            };
            if let Some(column) = column {
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
        } else if let Some(message) = kind_change_error(lookups, asset, new_kind, model_changes) {
            error("kind", message);
            kind_ok = false;
        }
    }

    // Attributes, read as the row's kind types them and merged into what
    // the asset holds.
    let mut attributes = None;
    let mut attributes_changed = false;
    let mut dropped = Vec::new();
    if let Some(kind) = &kind {
        let read = read_attributes(row, lookups, kind, options);
        dropped = read.dropped;
        let read_ok = read.errors.is_empty();
        for (column, message) in read.errors {
            error(&column, message);
        }
        let mut merged = existing
            .and_then(|a| a.attributes.as_object().cloned())
            .unwrap_or_default();
        let mut writes = !read.values.is_empty();
        for (key, value) in read.values {
            if merged.get(&key) != Some(&value) {
                attributes_changed = true;
            }
            merged.insert(key, value);
        }
        // A model the row links the asset to fills in its defaults where
        // the asset has no value, as the asset page's stamp does.
        if let (true, Some(model)) = (model_changes, linked) {
            let stamped =
                stamp_default_attributes(&Value::Object(merged.clone()), &model.default_attributes);
            if let Value::Object(stamped) = stamped {
                if stamped != merged {
                    merged = stamped;
                    writes = true;
                    attributes_changed = true;
                }
            }
        }
        let kind_changed = existing.is_some_and(|a| a.kind != *kind);
        // A new asset is always checked; an existing one when the row
        // writes an attribute or changes its kind, as PUT checks it.
        if read_ok && kind_ok && (existing.is_none() || writes || kind_changed) {
            if let Some(info) = lookups.by_slug.get(kind) {
                let merged_value = Value::Object(merged.clone());
                if let Err(e) = validate_attributes(&info.schema, &merged_value) {
                    let (key, message) = attribute_error(&e, lookups, kind, &merged);
                    // Without a key the attributes as a whole are refused,
                    // which the kind accounts for.
                    let column =
                        key.map_or_else(|| "kind".to_string(), |k| lookups.attribute_id(&k));
                    error(&column, message);
                }
            }
        }
        if writes {
            attributes = Some(Value::Object(merged));
        }
    }

    dropped.extend(group_drops);

    let synced_writes = attributes_changed || groups_change;
    match kind {
        Some(kind) if errors.is_empty() => {
            let action = match target {
                Some(id) if synced && (synced_writes || status_to.is_some()) => {
                    RowAction::SyncedUpdate(id)
                }
                Some(id) if synced => RowAction::Unchanged(id),
                Some(id) => RowAction::Update(id),
                None => RowAction::Create,
            };
            Ok(PlannedRow {
                action,
                kind,
                attributes,
                purchase_date,
                primary_user,
                dropped,
                maker,
                model,
                model_changes,
                groups,
                location,
                location_snapped,
                status_to,
                synced_writes,
            })
        }
        _ => Err(errors),
    }
}

/// The status a row moves its asset to, checked as
/// [`asset_lifecycle_repo::transition`] checks it so the commit's
/// transition goes through: `None` for a blank cell or the status the asset
/// already has (a new asset starts in service). Disposed and on loan come
/// from a disposal or a loan; a disposed asset keeps its status, and one
/// out on loan leaves it only by its return. The error, worded for the
/// admin.
fn plan_status(
    row: &HashMap<String, String>,
    existing: Option<&Asset>,
    lookups: &Lookups,
) -> Result<Option<AssetStatus>, String> {
    let cell = trimmed(row, "status");
    if cell.is_empty() {
        return Ok(None);
    }
    let to = read_status(&cell).ok_or_else(|| format!("'{cell}' isn't a status"))?;
    let from = existing.map_or(AssetStatus::default_str(), |a| a.status.as_str());
    if to.as_str() == from {
        return Ok(None);
    }
    let on_loan = existing.is_some_and(|a| lookups.on_loan.contains(&a.id));
    match to {
        AssetStatus::Disposed => {
            return Err(format!(
                "'{cell}' is set by recording a disposal on the asset page"
            ));
        }
        _ if from == AssetStatus::Disposed.as_str() => {
            return Err(disposal_is_final(existing.map_or("", |a| a.name.as_str())));
        }
        _ if from == AssetStatus::OnLoan.as_str() && on_loan => {
            return Err(out_on_loan(existing.map_or("", |a| a.name.as_str())));
        }
        AssetStatus::OnLoan if !on_loan => {
            return Err(format!(
                "'{cell}' is set by issuing a loan on the asset page"
            ));
        }
        _ => {}
    }
    Ok(Some(to))
}

fn disposal_is_final(name: &str) -> String {
    format!("{name} is disposed, and disposal is final; its status can't change")
}

fn out_on_loan(name: &str) -> String {
    format!("{name} is out on loan; return the loan on the asset page to change its status")
}

/// The commit's status move for a row, through the lifecycle rules: reason
/// Imported, naming the job. The asset as it stands after, and the event;
/// no event when the asset already has the status.
fn move_status(
    conn: &mut DbConnection,
    asset: Asset,
    to: AssetStatus,
    ctx: &ImportContext,
    row: usize,
) -> Result<(Asset, Option<AssetLifecycleEvent>), CommitError> {
    let input = asset_lifecycle_repo::TransitionInput {
        asset_id: asset.id,
        to_status: to.as_str().to_string(),
        reason: Some(asset_lifecycle_repo::IMPORTED_REASON.to_string()),
        ticket_id: None,
        metadata: serde_json::json!({ "import_job_id": ctx.job_id }),
        actor_uuid: ctx.actor,
        disposal: None,
    };
    let name = asset.name.clone();
    let refused = |message: String| CommitError::Row {
        row,
        column: Some("status".to_string()),
        message,
    };
    match asset_lifecycle_repo::transition(conn, input) {
        Ok((moved, event)) => Ok((moved, Some(event))),
        // Moved there since the plan was checked: nothing to write.
        Err(TransitionError::NoOp) => {
            let current = asset_repo::get_device_by_id(conn, asset.id)?;
            Ok((current, None))
        }
        Err(TransitionError::FromDisposed) => Err(refused(disposal_is_final(&name))),
        Err(TransitionError::OnLoanReturnViaLoan) => Err(refused(out_on_loan(&name))),
        Err(TransitionError::OnLoanNeedsActiveLoan) => Err(refused(
            "On loan is set by issuing a loan on the asset page".to_string(),
        )),
        Err(TransitionError::Db(e)) => Err(row_write_error(row, e)),
        Err(TransitionError::AssetNotFound) => {
            Err(CommitError::Db(diesel::result::Error::NotFound))
        }
        Err(
            TransitionError::DisposalRequired
            | TransitionError::DisposalNotAllowed
            | TransitionError::InvalidSanitizationMethod(_),
        ) => Err(refused(
            "Disposed is set by recording a disposal on the asset page".to_string(),
        )),
    }
}

/// The context family attributes are answered per: the row's kind.
const KIND_FAMILY: &str = "kind";
/// The context family models are answered per: the row's manufacturer.
const MANUFACTURER_FAMILY: &str = "manufacturer";

/// The catalog a field's values link to.
fn lookup_kind(field: &str) -> Option<LookupKind> {
    match field {
        "manufacturer" => Some(LookupKind::Manufacturer),
        "model" => Some(LookupKind::Model),
        "groups" => Some(LookupKind::Group),
        _ => None,
    }
}

/// The answer Match values gives the rows holding `value` of a lookup
/// field in `context`.
fn lookup_answer<'a>(
    options: &'a ImportOptions,
    field: &str,
    context: &str,
    value: &str,
) -> Option<&'a ValueAnswer> {
    options
        .lookups
        .get(field)?
        .get(context)?
        .get(&normalize_value(value))
}

/// The catalog entry a link answer names, by id.
fn linked_id(answer: Option<&ValueAnswer>) -> Option<i32> {
    match answer? {
        ValueAnswer::Match { target } => match target {
            Value::String(s) => s.parse().ok(),
            Value::Number(n) => n.as_i64().and_then(|n| i32::try_from(n).ok()),
            _ => None,
        },
        _ => None,
    }
}

/// What a row's manufacturer cell comes to.
#[derive(Debug, Clone, PartialEq)]
enum MakerPick {
    /// Blank, or left blank: nothing is written.
    None,
    /// Kept as text, unlinked.
    Text(String),
    /// The catalog's manufacturer.
    Link(i32),
    /// Created by the import; `key` is its name normalized, `text` the
    /// row's spelling.
    New { key: String, text: String },
    /// Names more than one manufacturer; the row is skipped.
    Ambiguous(String),
}

impl MakerPick {
    /// The manufacturer context the row's model is matched in: the name of
    /// the manufacturer the row has, normalized, or empty for none.
    fn context(&self, lookups: &Lookups) -> String {
        match self {
            Self::None | Self::Ambiguous(_) => String::new(),
            Self::Text(text) => normalize_value(text),
            Self::Link(id) => lookups
                .catalog
                .manufacturer(*id)
                .map(|m| normalize_value(&m.name))
                .unwrap_or_default(),
            Self::New { key, .. } => key.clone(),
        }
    }
}

/// What a row's model cell comes to.
#[derive(Debug, Clone, PartialEq)]
enum ModelPick {
    None,
    Text(String),
    Link(i32),
    New { key: String, text: String },
}

/// A group a row lists.
#[derive(Debug, Clone, PartialEq)]
enum GroupPick {
    Link(i32),
    New { key: String },
}

/// The row's manufacturer: the Match values answer for its cell, else the
/// one manufacturer named so; a name no manufacturer has is kept as text.
fn pick_maker(
    row: &HashMap<String, String>,
    lookups: &Lookups,
    options: &ImportOptions,
) -> MakerPick {
    let cell = trimmed(row, "manufacturer");
    if cell.is_empty() {
        return MakerPick::None;
    }
    let key = normalize_value(&cell);
    let answer = lookup_answer(options, "manufacturer", "", &cell);
    if let Some(id) = linked_id(answer).filter(|id| lookups.catalog.manufacturer(*id).is_some()) {
        return MakerPick::Link(id);
    }
    let named = lookups.catalog.manufacturers_named(&key);
    match (answer, named.as_slice()) {
        (Some(ValueAnswer::KeepText), _) => MakerPick::Text(cell),
        (Some(ValueAnswer::LeaveBlank), _) => MakerPick::None,
        (_, [only]) => MakerPick::Link(only.id),
        (Some(ValueAnswer::Create), []) => MakerPick::New { key, text: cell },
        (_, []) => MakerPick::Text(cell),
        (_, several) => MakerPick::Ambiguous(format!(
            "'{cell}' matches more than one manufacturer: {}",
            several
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The row's model within its manufacturer `context`: the Match values
/// answer for its cell, else the one model named so there. With no
/// manufacturer the name must be one model's across the catalog. A name no
/// model has is kept as text. The error, when the name is ambiguous.
fn pick_model(
    row: &HashMap<String, String>,
    lookups: &Lookups,
    options: &ImportOptions,
    context: &str,
) -> (ModelPick, Option<String>) {
    let cell = trimmed(row, "model");
    if cell.is_empty() {
        return (ModelPick::None, None);
    }
    let key = normalize_value(&cell);
    let answer = lookup_answer(options, "model", context, &cell);
    if let Some(id) = linked_id(answer).filter(|id| lookups.catalog.model(*id).is_some()) {
        return (ModelPick::Link(id), None);
    }
    let named = lookups.catalog.models_named(context, &key);
    match (answer, named.as_slice()) {
        (Some(ValueAnswer::KeepText), _) => (ModelPick::Text(cell), None),
        (Some(ValueAnswer::LeaveBlank), _) => (ModelPick::None, None),
        (_, [only]) => (ModelPick::Link(only.id), None),
        (Some(ValueAnswer::Create), []) => (ModelPick::New { key, text: cell }, None),
        (_, []) => (ModelPick::Text(cell), None),
        (_, several) => {
            let message = if context.is_empty() {
                let mut makers: Vec<&str> = several
                    .iter()
                    .map(|m| lookups.catalog.maker_of(m))
                    .collect();
                makers.sort_unstable();
                makers.dedup();
                format!(
                    "'{cell}' is a model of more than one manufacturer ({}); give the row a manufacturer",
                    makers.join(", ")
                )
            } else {
                format!(
                    "'{cell}' matches more than one model: {}",
                    several
                        .iter()
                        .map(|m| m.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            (ModelPick::Text(cell), Some(message))
        }
    }
}

/// The groups a row lists (`;`-separated): linked to the active group of
/// each name, or created where Match values says so. A name no group has,
/// a directory group's, and one left off are left out and counted. The
/// errors, for a name too long to create.
fn pick_groups(
    row: &HashMap<String, String>,
    lookups: &Lookups,
    options: &ImportOptions,
    kind: &str,
) -> (Vec<GroupPick>, Vec<DroppedCell>, Vec<String>) {
    let cell = trimmed(row, "groups");
    let mut picks: Vec<GroupPick> = Vec::new();
    let mut dropped = Vec::new();
    let mut errors = Vec::new();
    for item in values::list_items(&cell) {
        let key = normalize_value(item);
        let answer = lookup_answer(options, "groups", "", item);
        let linked = linked_id(answer)
            .filter(|id| lookups.catalog.group(*id).is_some())
            .or_else(|| lookups.catalog.group_named(&key).map(|g| g.id));
        let pick = match (answer, linked) {
            (Some(ValueAnswer::LeaveBlank | ValueAnswer::KeepText), _) => None,
            (_, Some(id)) => Some(GroupPick::Link(id)),
            (Some(ValueAnswer::Create), None) => {
                if item.chars().count() > GROUP_NAME_MAX {
                    errors.push(format!(
                        "'{item}' is longer than {GROUP_NAME_MAX} characters, the most a group name holds"
                    ));
                    None
                } else {
                    Some(GroupPick::New { key })
                }
            }
            _ => None,
        };
        match pick {
            Some(pick) => {
                if !picks.contains(&pick) {
                    picks.push(pick);
                }
            }
            None => dropped.push(DroppedCell {
                column: "groups".to_string(),
                source_column: None,
                kind: kind.to_string(),
                value: Some(item.to_string()),
            }),
        }
    }
    (picks, dropped, errors)
}

/// Longest group name the database holds.
const GROUP_NAME_MAX: usize = 255;

/// The manufacturer, model and location text a row writes, as the plan
/// knows them: a created entry is named as this row spells it.
struct PlannedText {
    maker: Option<String>,
    model: Option<String>,
    location: Option<String>,
}

/// The manufacturer text a row writes: a linked model's maker, else the
/// manufacturer's name or the cell.
fn maker_text(maker: &MakerPick, model: &ModelPick, lookups: &Lookups) -> Option<String> {
    if let Some(linked) = match model {
        ModelPick::Link(id) => lookups.catalog.model(*id),
        _ => None,
    } {
        return Some(lookups.catalog.maker_of(linked).to_string());
    }
    match maker {
        MakerPick::None | MakerPick::Ambiguous(_) => None,
        MakerPick::Text(text) | MakerPick::New { text, .. } => Some(text.clone()),
        MakerPick::Link(id) => lookups.catalog.manufacturer(*id).map(|m| m.name.clone()),
    }
}

fn model_text(model: &ModelPick, lookups: &Lookups) -> Option<String> {
    match model {
        ModelPick::None => None,
        ModelPick::Text(text) | ModelPick::New { text, .. } => Some(text.clone()),
        ModelPick::Link(id) => lookups.catalog.model(*id).map(|m| m.name.clone()),
    }
}

/// A manufacturer a created model belongs to: one the catalog has, or one
/// the same import creates (by its key).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum MakerRef {
    Existing(i32),
    New(String),
}

/// The manufacturer a row's created model belongs to.
fn maker_ref(maker: &MakerPick) -> Option<MakerRef> {
    match maker {
        MakerPick::Link(id) => Some(MakerRef::Existing(*id)),
        MakerPick::New { key, .. } => Some(MakerRef::New(key.clone())),
        _ => None,
    }
}

/// The names created entries take: each as the file first spells it.
#[derive(Default)]
struct Names {
    makers: HashMap<String, String>,
    models: HashMap<(MakerRef, String), String>,
    groups: HashMap<String, String>,
}

/// One data row and its plan.
struct Entry {
    source_row: usize,
    row: HashMap<String, String>,
    planned: Result<PlannedRow, Vec<(Option<String>, String)>>,
}

/// Every row's plan, with what only the file as a whole can say.
struct PlannedFile {
    entries: Vec<Entry>,
    names: Names,
    catalog: CatalogPlan,
    /// Rows whose location takes an existing spelling.
    locations_snapped: usize,
}

/// Plan every row, in file order, then hold up the models the file creates
/// against each other: rows creating one model under different kinds are
/// skipped, naming each other. What's left gives the catalog entries the
/// import creates.
fn plan_all(parsed: &ParsedCsv, lookups: &Lookups, options: &ImportOptions) -> PlannedFile {
    let file_keys = FileKeys::scan(parsed);
    let mut entries: Vec<Entry> = parsed
        .records()
        .map(|record| {
            let row = with_constants(record.values, lookups, options);
            let planned = plan_row(&row, lookups, &file_keys, options);
            Entry {
                source_row: record.source_row,
                row,
                planned,
            }
        })
        .collect();

    // The kinds each created model is asked for, and by which rows.
    let mut kinds: HashMap<(MakerRef, String), BTreeMap<String, Vec<usize>>> = HashMap::new();
    for entry in &entries {
        let Ok(planned) = &entry.planned else {
            continue;
        };
        if let (ModelPick::New { key, .. }, Some(maker)) =
            (&planned.model, maker_ref(&planned.maker))
        {
            kinds
                .entry((maker, key.clone()))
                .or_default()
                .entry(planned.kind.clone())
                .or_default()
                .push(entry.source_row);
        }
    }
    for entry in &mut entries {
        let Ok(planned) = &entry.planned else {
            continue;
        };
        let ModelPick::New { key, text } = &planned.model else {
            continue;
        };
        let Some(maker) = maker_ref(&planned.maker) else {
            continue;
        };
        let Some(asked) = kinds.get(&(maker, key.clone())).filter(|k| k.len() > 1) else {
            continue;
        };
        let listed: Vec<String> = asked
            .iter()
            .map(|(kind, rows)| format!("{} ({})", lookups.kind_label(kind), rows_text(rows)))
            .collect();
        let message = format!(
            "'{text}' would be created as more than one kind: {}; give these rows one kind",
            listed.join(", ")
        );
        entry.planned = Err(vec![(Some("model".to_string()), message)]);
    }

    // Created entries, named as the file first spells them.
    let mut names = Names::default();
    let mut catalog = CatalogPlan::default();
    let mut locations_snapped = 0;
    for entry in &entries {
        let Ok(planned) = &entry.planned else {
            continue;
        };
        if planned.location_snapped {
            locations_snapped += 1;
        }
        if let MakerPick::New { key, text } = &planned.maker {
            if !names.makers.contains_key(key) {
                names.makers.insert(key.clone(), text.clone());
                catalog.manufacturers.push(text.clone());
            }
        }
        if let (ModelPick::New { key, text }, Some(maker)) =
            (&planned.model, maker_ref(&planned.maker))
        {
            let id = (maker.clone(), key.clone());
            if let std::collections::hash_map::Entry::Vacant(slot) = names.models.entry(id) {
                slot.insert(text.clone());
                let manufacturer = match &maker {
                    MakerRef::Existing(id) => lookups
                        .catalog
                        .manufacturer(*id)
                        .map(|m| m.name.clone())
                        .unwrap_or_default(),
                    MakerRef::New(key) => names.makers.get(key).cloned().unwrap_or_default(),
                };
                catalog.models.push(PlannedModel {
                    manufacturer,
                    name: text.clone(),
                    kind: planned.kind.clone(),
                });
            }
        }
        for group in &planned.groups {
            if let GroupPick::New { key } = group {
                if !names.groups.contains_key(key) {
                    let text = first_item(&entry.row, key);
                    names.groups.insert(key.clone(), text.clone());
                    catalog.groups.push(text);
                }
            }
        }
    }
    PlannedFile {
        entries,
        names,
        catalog,
        locations_snapped,
    }
}

/// The spelling of the group item `key` in a row's groups cell.
fn first_item(row: &HashMap<String, String>, key: &str) -> String {
    let cell = trimmed(row, "groups");
    let found = values::list_items(&cell)
        .find(|item| normalize_value(item) == key)
        .unwrap_or(key)
        .to_string();
    found
}

/// File lines for a message: "row 5", "rows 2 and 4", "rows 2, 4 and 9".
fn rows_text(rows: &[usize]) -> String {
    let mut listed: Vec<String> = rows.iter().map(ToString::to_string).collect();
    match listed.len() {
        0 => String::new(),
        1 => format!("row {}", listed[0]),
        _ => {
            let last = listed.pop().unwrap_or_default();
            format!("rows {} and {last}", listed.join(", "))
        }
    }
}

fn location_notice(count: usize) -> Notice {
    let message = if count == 1 {
        "1 location matched an existing spelling.".to_string()
    } else {
        format!("{count} locations matched an existing spelling.")
    };
    Notice {
        code: "location_snapped".to_string(),
        count,
        message,
    }
}

/// What a row writes once its catalog entries exist.
struct Texts {
    maker: Option<String>,
    model: Option<String>,
    /// The model to link the asset to, when the link changes.
    link: Option<i32>,
    /// The group ids the row lists.
    groups: Vec<i32>,
}

/// Catalog entries the commit has created, by key, so each is created once.
#[derive(Default)]
struct Made {
    makers: HashMap<String, Manufacturer>,
    models: HashMap<(MakerRef, String), AssetModel>,
    groups: HashMap<String, AssetGroup>,
}

impl Made {
    /// Create what the row needs and the catalog lacks, through the
    /// repositories, recording each on `created`, and say what the row
    /// writes.
    fn settle(
        &mut self,
        conn: &mut DbConnection,
        lookups: &Lookups,
        names: &Names,
        planned: &PlannedRow,
        actor: Option<Uuid>,
        created: &mut Vec<CatalogRef>,
    ) -> Result<Texts, diesel::result::Error> {
        let mut maker = maker_text(&planned.maker, &planned.model, lookups);
        let mut maker_id = match &planned.maker {
            MakerPick::Link(id) => Some(*id),
            _ => None,
        };
        if let MakerPick::New { key, text } = &planned.maker {
            let made = match self.makers.get(key) {
                Some(m) => m.clone(),
                None => {
                    let name = names.makers.get(key).unwrap_or(text).clone();
                    let m = manufacturer_repo::create(
                        conn,
                        NewManufacturer {
                            name,
                            created_by: actor,
                        },
                    )?;
                    created.push(CatalogRef {
                        kind: CatalogKind::Manufacturer,
                        id: m.id,
                        observed_updated_at: Some(observed_timestamp(m.updated_at.naive_utc())),
                    });
                    self.makers.insert(key.clone(), m.clone());
                    m
                }
            };
            maker = Some(made.name.clone());
            maker_id = Some(made.id);
        }

        let mut model = model_text(&planned.model, lookups);
        let mut link = match planned.model {
            ModelPick::Link(id) if planned.model_changes => Some(id),
            _ => None,
        };
        if let (ModelPick::New { key, text }, Some(owner), Some(maker_id)) =
            (&planned.model, maker_ref(&planned.maker), maker_id)
        {
            let id = (owner, key.clone());
            let made = match self.models.get(&id) {
                Some(m) => m.clone(),
                None => {
                    let name = names.models.get(&id).unwrap_or(text).clone();
                    let m = model_repo::create(
                        conn,
                        NewAssetModel {
                            manufacturer_id: maker_id,
                            name,
                            kind: planned.kind.clone(),
                            part_number: None,
                            default_attributes: serde_json::json!({}),
                            notes: None,
                            created_by: actor,
                        },
                    )?;
                    created.push(CatalogRef {
                        kind: CatalogKind::Model,
                        id: m.id,
                        observed_updated_at: Some(observed_timestamp(m.updated_at.naive_utc())),
                    });
                    self.models.insert(id, m.clone());
                    m
                }
            };
            model = Some(made.name.clone());
            link = Some(made.id);
        }

        let mut groups = Vec::with_capacity(planned.groups.len());
        for group in &planned.groups {
            match group {
                GroupPick::Link(id) => groups.push(*id),
                GroupPick::New { key } => {
                    let made = match self.groups.get(key) {
                        Some(g) => g.clone(),
                        None => {
                            let name = names
                                .groups
                                .get(key)
                                .cloned()
                                .unwrap_or_else(|| key.clone());
                            let g = group_repo::create_group(
                                conn,
                                NewAssetGroup {
                                    name,
                                    description: None,
                                    color: None,
                                    display_order: 0,
                                    created_by: actor,
                                },
                            )?;
                            created.push(CatalogRef {
                                kind: CatalogKind::Group,
                                id: g.id,
                                observed_updated_at: Some(observed_timestamp(g.updated_at)),
                            });
                            self.groups.insert(key.clone(), g.clone());
                            g
                        }
                    };
                    if !groups.contains(&made.id) {
                        groups.push(made.id);
                    }
                }
            }
        }
        Ok(Texts {
            maker,
            model,
            link,
            groups,
        })
    }
}

/// What one row's attribute cells come to.
struct ReadAttributes {
    /// Key to value, typed.
    values: Map<String, Value>,
    /// Field id and why its cell doesn't read.
    errors: Vec<(String, String)>,
    dropped: Vec<DroppedCell>,
}

/// Read a row's attribute cells as `kind`'s schema types them. A value for
/// an attribute the kind doesn't have, or has only as a synced field, is
/// dropped; one a constant set is dropped without a count, since the file
/// didn't hold it.
fn read_attributes(
    row: &HashMap<String, String>,
    lookups: &Lookups,
    kind: &str,
    options: &ImportOptions,
) -> ReadAttributes {
    let mut out = ReadAttributes {
        values: Map::new(),
        errors: Vec::new(),
        dropped: Vec::new(),
    };
    for def in &lookups.attributes {
        let raw = trimmed(row, &def.id);
        if raw.is_empty() {
            continue;
        }
        let Some(prop) = def.prop(kind) else {
            if !options.constants.iter().any(|c| c.field == def.id) {
                out.dropped.push(DroppedCell {
                    column: def.id.clone(),
                    source_column: None,
                    kind: kind.to_string(),
                    value: None,
                });
            }
            continue;
        };
        match parse_attribute(&raw, prop, lookups, options) {
            Ok(value) => {
                out.values.insert(def.key.clone(), value);
            }
            Err(message) => out.errors.push((def.id.clone(), message)),
        }
    }
    out
}

/// One attribute cell as its property's schema types it. The schema's
/// other rules (length, pattern, range) are checked with the whole row.
fn parse_attribute(
    raw: &str,
    prop: &Value,
    lookups: &Lookups,
    options: &ImportOptions,
) -> Result<Value, String> {
    match prop.get("type").and_then(Value::as_str).unwrap_or("string") {
        "array" => {
            let items = prop.get("items").cloned().unwrap_or(Value::Null);
            raw.split(';')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| parse_attribute(item, &items, lookups, options))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        "boolean" => values::read_boolean(raw)
            .map(Value::Bool)
            .ok_or_else(|| format!("'{raw}' is not yes or no")),
        ty @ ("number" | "integer") => {
            let number = values::parse_decimal(raw, options.decimal_mark())?
                .ok_or_else(|| format!("'{raw}' is not a number"))?;
            number_value(&number, ty == "integer").ok_or_else(|| {
                if ty == "integer" {
                    format!("'{raw}' is not a whole number")
                } else {
                    format!("'{raw}' is not a number")
                }
            })
        }
        _ => parse_text_attribute(raw, prop, lookups, options).map(Value::String),
    }
}

/// A number as JSON: whole numbers as integers.
fn number_value(number: &BigDecimal, integer: bool) -> Option<Value> {
    if number.is_integer() {
        return number.to_i64().map(Value::from);
    }
    if integer {
        return None;
    }
    let float: f64 = number.to_string().parse().ok()?;
    serde_json::Number::from_f64(float).map(Value::Number)
}

/// A text attribute cell: an enum option in its own spelling, a date, a
/// date-time, a member, an asset, or the text as it is.
fn parse_text_attribute(
    raw: &str,
    prop: &Value,
    lookups: &Lookups,
    options: &ImportOptions,
) -> Result<String, String> {
    let choices = enum_options(prop);
    if prop.get("enum").is_some() {
        let wanted = values::loose(raw);
        return choices
            .iter()
            .find(|c| values::loose(c) == wanted)
            .cloned()
            .ok_or_else(|| format!("'{raw}' is not one of: {}", choices.join(", ")));
    }
    let order = options.date_order;
    match prop.get("format").and_then(Value::as_str) {
        Some("date") => values::parse_date(raw, order)
            .map(|d| d.format("%Y-%m-%d").to_string())
            .ok_or_else(|| invalid_date(raw, order)),
        Some("date-time") => parse_date_time(raw, order).ok_or_else(|| {
            format!(
                "'{raw}' is not a valid date and time; use YYYY-MM-DDTHH:MM:SSZ or {}",
                values::date_hint(order)
            )
        }),
        Some("user-uuid") => lookups
            .member(raw)
            .map(|u| u.to_string())
            .ok_or_else(|| not_a_member(raw)),
        Some("asset-ref") => asset_ref(raw, lookups),
        _ => Ok(raw.to_string()),
    }
}

/// An RFC 3339 date-time, a date-time without an offset (read as UTC), or a
/// date (midnight UTC), as RFC 3339.
fn parse_date_time(raw: &str, order: DateOrder) -> Option<String> {
    use chrono::SecondsFormat;
    if let Ok(at) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(at.to_rfc3339_opts(SecondsFormat::AutoSi, true));
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(at) = chrono::NaiveDateTime::parse_from_str(raw, format) {
            return Some(at.and_utc().to_rfc3339_opts(SecondsFormat::AutoSi, true));
        }
    }
    let date = values::parse_date(raw, order)?;
    Some(
        date.and_hms_opt(0, 0, 0)?
            .and_utc()
            .to_rfc3339_opts(SecondsFormat::AutoSi, true),
    )
}

/// An asset reference: a tag one asset holds, or an asset id.
fn asset_ref(raw: &str, lookups: &Lookups) -> Result<String, String> {
    match lookups.tags.get(raw).map(Vec::as_slice) {
        Some([id]) => return Ok(id.to_string()),
        Some(ids) if ids.len() > 1 => return Err(shared_tag_error(lookups, raw, ids)),
        _ => {}
    }
    match raw.parse::<i32>() {
        Ok(id) if id > 0 => Ok(id.to_string()),
        _ => Err(format!("no asset has the tag '{raw}'")),
    }
}

fn invalid_date(raw: &str, order: DateOrder) -> String {
    format!(
        "'{raw}' is not a valid date; use {}",
        values::date_hint(order)
    )
}

fn not_a_member(raw: &str) -> String {
    format!("'{raw}' is not a member of this workspace")
}

/// An attribute check that failed, as the key it's about (when one) and a
/// message for the admin.
fn attribute_error(
    e: &AttributeError,
    lookups: &Lookups,
    kind: &str,
    attributes: &Map<String, Value>,
) -> (Option<String>, String) {
    use AttributeError as E;
    let shown = |key: &str| attributes.get(key).and_then(value_text).unwrap_or_default();
    let kind_label = lookups.kind_label(kind);
    let about = |key: &String, message: String| (Some(key.clone()), message);
    match e {
        E::NotObject => (None, "attributes must be an object".to_string()),
        // Shown after the attribute's name: "Fuel: required for Vehicle
        // assets".
        E::MissingRequired(key) => about(key, format!("required for {kind_label} assets")),
        E::UnknownProperty(key) => about(
            key,
            format!("{kind_label} assets have no attribute '{key}'; remove it on the asset page"),
        ),
        E::WrongType {
            property, expected, ..
        } => about(
            property,
            format!("'{}' is not a {expected}", shown(property)),
        ),
        E::NotInEnum { property, allowed } => {
            let allowed: Vec<String> = allowed.iter().filter_map(value_text).collect();
            about(
                property,
                format!(
                    "'{}' is not one of: {}",
                    shown(property),
                    allowed.join(", ")
                ),
            )
        }
        E::TooShort { property, min } => about(
            property,
            format!("'{}' is shorter than {min} characters", shown(property)),
        ),
        E::TooLong { property, max } => about(
            property,
            format!("'{}' is longer than {max} characters", shown(property)),
        ),
        E::PatternMismatch { property } => about(
            property,
            format!(
                "'{}' doesn't match the pattern {kind_label} sets for it",
                shown(property)
            ),
        ),
        E::BadFormat { property, format } => about(
            property,
            format!("'{}' is not a valid {format}", shown(property)),
        ),
        E::BelowMinimum { property, min } => about(
            property,
            format!("'{}' is below the minimum of {min}", shown(property)),
        ),
        E::AboveMaximum { property, max } => about(
            property,
            format!("'{}' is above the maximum of {max}", shown(property)),
        ),
        E::NotMultipleOf { property, step } => about(
            property,
            format!("'{}' is not a multiple of {step}", shown(property)),
        ),
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
/// asset. PUT takes none of these on a synced asset; attributes it does
/// take. Values compare as the update would write them, so a blank cell
/// never differs, and stored values compare trimmed, so a re-imported
/// export matches. Manufacturer, model and location compare as the row
/// writes them (`texts`): linked, or snapped to a stored spelling.
fn synced_column_change(
    row: &HashMap<String, String>,
    asset: &Asset,
    new_kind: &str,
    mark: DecimalMark,
    purchase_date: Option<NaiveDate>,
    primary_user: Option<Uuid>,
    texts: &PlannedText,
) -> Option<&'static str> {
    fn stored(value: &Option<String>) -> Option<&str> {
        value.as_deref().map(str::trim).filter(|v| !v.is_empty())
    }
    // A blank cell leaves any column as it is.
    let kept = |column: &str, current: &Option<String>| {
        opt_string(row, column).is_some_and(|v| Some(v.as_str()) != stored(current))
    };
    let written = |value: &Option<String>, current: &Option<String>| {
        value
            .as_deref()
            .is_some_and(|v| Some(v.trim()) != stored(current))
    };
    let kept_decimal = |column: &str, current: &Option<BigDecimal>| {
        check_decimal(row, column, mark).is_ok_and(|v| v.is_some_and(|v| Some(v) != *current))
    };
    let changed: [(&'static str, bool); 12] = [
        ("name", trimmed(row, "name") != asset.name.trim()),
        ("kind", new_kind != asset.kind),
        ("serial_number", kept("serial_number", &asset.serial_number)),
        ("manufacturer", written(&texts.maker, &asset.manufacturer)),
        ("model", written(&texts.model, &asset.model)),
        ("location", written(&texts.location, &asset.location)),
        ("notes", kept("notes", &asset.notes)),
        ("quantity", kept_decimal("quantity", &asset.quantity)),
        ("unit", kept("unit", &asset.unit)),
        (
            "low_stock_threshold",
            kept_decimal("low_stock_threshold", &asset.low_stock_threshold),
        ),
        (
            "purchase_date",
            purchase_date.is_some_and(|d| Some(d) != asset.purchase_date),
        ),
        (
            "primary_user",
            primary_user.is_some_and(|u| Some(u) != asset.primary_user_uuid),
        ),
    ];
    changed
        .into_iter()
        .find_map(|(column, changed)| changed.then_some(column))
}

/// The error for an update that moves `existing` (not synced) to
/// `new_kind`, if the asset page wouldn't allow it: its model sets its
/// kind (unless the row links it to another model, which is what changes
/// the kind), or it holds attributes the new kind doesn't define.
fn kind_change_error(
    lookups: &Lookups,
    existing: &Asset,
    new_kind: &str,
    model_changes: bool,
) -> Option<String> {
    let old_kind = &existing.kind;
    if old_kind == new_kind {
        return None;
    }
    if let (Some(model_id), false) = (existing.model_id, model_changes) {
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
    Some(format!(
        "changing kind from {} to {} would drop attributes: {}; change the kind on the asset page instead",
        lookups.kind_label(old_kind),
        lookups.kind_label(new_kind),
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

/// A decimal cell, read as [`values::parse_decimal`] reads it.
fn check_decimal(
    row: &HashMap<String, String>,
    key: &str,
    mark: DecimalMark,
) -> Result<Option<BigDecimal>, String> {
    values::parse_decimal(&trimmed(row, key), mark)
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
    planned: &PlannedRow,
    texts: &Texts,
    created_by: Option<Uuid>,
    mark: DecimalMark,
) -> NewAsset {
    NewAsset {
        name: trimmed(row, "name"),
        serial_number: opt_string(row, "serial_number"),
        manufacturer: texts.maker.clone(),
        model: texts.model.clone(),
        location: planned.location.clone(),
        notes: opt_string(row, "notes"),
        primary_user_uuid: planned.primary_user,
        purchase_date: planned.purchase_date,
        asset_tag: opt_string(row, "asset_tag"),
        kind: planned.kind.clone(),
        attributes: planned
            .attributes
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
        quantity: opt_decimal(row, "quantity", mark),
        unit: opt_string(row, "unit"),
        external_sync_source: None,
        low_stock_threshold: opt_decimal(row, "low_stock_threshold", mark),
        created_by,
    }
}

fn build_asset_update(
    row: &HashMap<String, String>,
    planned: &PlannedRow,
    texts: &Texts,
    mark: DecimalMark,
) -> AssetUpdate {
    // A blank cell leaves the column as it is (outer `None`); it never clears.
    AssetUpdate {
        name: Some(trimmed(row, "name")),
        serial_number: opt_string(row, "serial_number").map(Some),
        manufacturer: texts.maker.clone(),
        model: texts.model.clone(),
        location: planned.location.clone().map(Some),
        notes: opt_string(row, "notes").map(Some),
        primary_user_uuid: planned.primary_user.map(Some),
        managed_by_user_uuid: None,
        purchase_date: planned.purchase_date.map(Some),
        asset_tag: opt_string(row, "asset_tag").map(Some),
        updated_at: Some(chrono::Utc::now().naive_utc()),
        kind: Some(planned.kind.clone()),
        attributes: planned.attributes.clone(),
        quantity: opt_decimal(row, "quantity", mark).map(Some),
        unit: opt_string(row, "unit").map(Some),
        external_sync_source: None,
        low_stock_threshold: opt_decimal(row, "low_stock_threshold", mark).map(Some),
        // Linked when the row links another model; otherwise the existing
        // link stays.
        model_id: texts.link.map(Some),
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
