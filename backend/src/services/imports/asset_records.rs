//! Asset records importer: years of service history from a spreadsheet.
//!
//! Each row is work done on an asset that already exists. The row finds its
//! asset by tag, by serial number, or by the tag and then the serial number
//! (`ImportOptions::match_by`); a row whose asset isn't found is an error,
//! grouped so the whole list of missing tags is one download, and the
//! import never creates an asset. A tag several assets hold, or a tag and a
//! serial that name different assets, is an error too.
//!
//! A row names its record type by slug or name. A built-in from the
//! catalogue the workspace hasn't added yet is added when the import is
//! committed (the plan lists it with the catalog entries); any other name
//! is an error. A row's outcome and the type's own fields are read as its
//! type defines them, and answered per type on Match values.
//!
//! With a `source_ref` column, a row whose reference an existing record
//! carries updates that record, under the mode the admin picked, so the same
//! file imported again changes nothing. A blank cell never changes a value.
//! Without one, a new row whose asset, type and day equal an existing
//! record's, and whose reference (else cost, else summary) does too, looks
//! already recorded: it is planned as unchanged and skipped, unless the
//! admin chooses to import those rows anyway.
//!
//! Every row is checked by the same rules a record entered by hand is
//! ([`record_repo::check_new`], [`record_repo::check_update`]), and written
//! through [`record_repo::create`] and [`record_repo::update`], with source
//! `import` and the job's id. Records carry no author: the job names who
//! imported them. The import never touches an asset: not its status, its
//! holder or its location.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use chrono_tz::Tz;
use diesel::prelude::*;
use serde_json::{Map, Value};
use unic_langid::LanguageIdentifier;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{Asset, AssetRecord, AssetRecordEdit, AssetRecordType, RecordSource};
use crate::repository::asset_record_types as type_repo;
use crate::repository::asset_records::{
    self as record_repo, Cost, NewRecord, RecordChanges, RecordError, RecordWarning, Snapshot,
    WorkDate, EARLIEST_DAY,
};
use crate::repository::workspaces as workspace_repo;
use crate::services::assets::record_catalogue::{self, CatalogueType};
use crate::services::custom_fields::schema::AttributeError;
use crate::utils::locale::local_moment;
use crate::utils::{i18n, money};

use super::assets::{
    enum_options, humanize, member_choices, number_value, value_text, value_type_of,
    ATTRIBUTE_PREFIX, NO_ASSET_WITH_SERIAL, NO_ASSET_WITH_TAG, NO_ASSET_WITH_THIS_SERIAL,
    NO_ASSET_WITH_THIS_TAG, NO_SERIAL_TO_MATCH, NO_TAG_TO_MATCH,
};
use super::csv_parser::ParsedCsv;
use super::previous::CurrentValues;
use super::types::{
    duplicate_in_file, BlankCompletion, CatalogPlan, CellError, CommitError, Committed, CostTotal,
    DateOrder, DroppedCell, FieldChange, FieldGroup, FieldSpec, HistoryFacts, ImportContext,
    ImportMode, ImportOptions, ImportType, ImportedRecords, Importer, MatchBy, Notice, RecordId,
    RowOutcome, RowPlan, RowResult, ValueAnswer, ValueType,
};
use super::values::{self, normalize_value, Choice, LookupKind, ValueContext};

/// The template's columns before a type's own fields, in order. A type's
/// field whose key is one of these is named `attr:<key>`.
pub const TEMPLATE_HEADERS: &[&str] = &[
    "asset_tag",
    "serial_number",
    "record_type",
    "occurred_on",
    "completed_on",
    "outcome",
    "summary",
    "notes",
    "vendor",
    "performed_by",
    "reference",
    "cost",
    "currency",
    "ticket",
    "holder",
    "location",
    "replacement_asset_tag",
    "source_ref",
];

/// The context family outcomes and a type's own fields are answered per.
const TYPE_FAMILY: &str = "record_type";

/// The group of rows whose tag and serial number both name no asset.
pub const NO_ASSET_WITH_THIS_TAG_OR_SERIAL: &str = "no asset with this tag or serial number";

/// Notice codes Review words.
pub const LIKELY_DUPLICATE: &str = "likely_duplicate";
pub const UNMATCHED_ASSETS: &str = "unmatched_assets";
pub const BEFORE_PURCHASE: &str = "before_purchase";
pub const COMPLETED_LONG_AFTER: &str = "completed_long_after";

static EN_US: LazyLock<LanguageIdentifier> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    "en-US".parse().expect("en-US parses")
});

/// A catalogue string in English: what the backend matches and reports in.
fn en(key: &str) -> Option<String> {
    let text = i18n::tr(&EN_US, key);
    (!text.starts_with('{')).then_some(text)
}

/// A record type rows can name: one of the workspace's, or a built-in from
/// the catalogue that the commit adds.
#[derive(Debug, Clone)]
struct TypeEntry {
    rtype: AssetRecordType,
    label: String,
    /// Not in the workspace yet: the commit adds it from the catalogue.
    from_catalogue: bool,
}

impl TypeEntry {
    fn slug(&self) -> &str {
        &self.rtype.slug
    }
}

/// A catalogue entry as the type it would be once added, with a stand-in id
/// below zero so it is told apart from the workspace's types.
fn catalogue_type(entry: &CatalogueType, id: i32) -> AssetRecordType {
    let now = Utc::now();
    AssetRecordType {
        id,
        workspace_id: 0,
        slug: entry.slug.to_string(),
        label: None,
        description: None,
        icon: Some(entry.icon.to_string()),
        color: None,
        field_schema: entry.field_schema(),
        field_order: entry.field_order().into_iter().map(Some).collect(),
        core_fields: entry.core_fields(),
        outcomes: entry.outcomes(),
        applies_to_kinds: None,
        opens_with_status: None,
        is_builtin: true,
        sort_order: entry.sort_order,
        archived_at: None,
        created_at: now,
        updated_at: now,
        created_by: None,
    }
}

/// A type's name in English: its own, else the catalogue's.
pub(super) fn type_label(rtype: &AssetRecordType) -> String {
    rtype
        .label
        .clone()
        .filter(|l| !l.trim().is_empty())
        .or_else(|| en(&format!("asset-record-type-{}", rtype.slug)))
        .unwrap_or_else(|| humanize(&rtype.slug))
}

/// An outcome's name in English: its own, else the catalogue's.
fn outcome_label(rtype: &AssetRecordType, key: &str, own: Option<&str>) -> String {
    own.map(str::to_string)
        .filter(|l| !l.trim().is_empty())
        .or_else(|| en(&format!("asset-record-outcome-{}-{key}", rtype.slug)))
        .unwrap_or_else(|| humanize(key))
}

/// The workspace's types, archived ones too, then the catalogue entries it
/// hasn't added.
fn load_types(conn: &mut DbConnection) -> Result<Vec<TypeEntry>, diesel::result::Error> {
    let mut out: Vec<TypeEntry> = type_repo::list(conn, true)?
        .into_iter()
        .map(|rtype| TypeEntry {
            label: type_label(&rtype),
            rtype,
            from_catalogue: false,
        })
        .collect();
    let mut stand_in = -1;
    for entry in record_catalogue::CATALOGUE {
        if out.iter().any(|t| t.slug() == entry.slug) {
            continue;
        }
        let rtype = catalogue_type(entry, stand_in);
        stand_in -= 1;
        out.push(TypeEntry {
            label: type_label(&rtype),
            rtype,
            from_catalogue: true,
        });
    }
    Ok(out)
}

/// What each type's records are called, by slug, as "Repair records", for
/// the error report.
pub fn type_owners(
    conn: &mut DbConnection,
) -> Result<HashMap<String, String>, diesel::result::Error> {
    Ok(load_types(conn)?
        .into_iter()
        .map(|t| (t.slug().to_string(), format!("{} records", t.label)))
        .collect())
}

/// The type a cell names, by slug or name, ignoring case, spacing, `_` and
/// `-`.
fn find_type<'a>(types: &'a [TypeEntry], cell: &str) -> Option<&'a TypeEntry> {
    let wanted = values::loose(cell);
    if wanted.is_empty() {
        return None;
    }
    types
        .iter()
        .find(|t| values::loose(t.slug()) == wanted)
        .or_else(|| types.iter().find(|t| values::loose(&t.label) == wanted))
}

/// One of a type's own fields a column can feed, across the types that
/// define it.
#[derive(Debug, Clone)]
struct FieldDef {
    key: String,
    /// Field id: the key, or `attr:<key>` when the key is a core column.
    id: String,
    title: String,
    /// Each type defining it: slug, name and the property's schema.
    types: Vec<(String, String, Value)>,
}

impl FieldDef {
    fn prop(&self, slug: &str) -> Option<&Value> {
        self.types
            .iter()
            .find(|(s, _, _)| s == slug)
            .map(|(_, _, prop)| prop)
    }

    fn field_spec(&self) -> FieldSpec {
        let first = self.types.first().map(|(_, _, prop)| prop);
        let value_type = first.map_or(ValueType::Text, value_type_of);
        let mut spec = attribute_spec(&self.id, &self.title, value_type);
        spec.kinds = Some(self.types.iter().map(|(s, _, _)| s.clone()).collect());
        if value_type == ValueType::Enum {
            let mut options: Vec<String> = Vec::new();
            for (_, _, prop) in &self.types {
                for option in enum_options(prop) {
                    if !options.contains(&option) {
                        options.push(option);
                    }
                }
            }
            spec.enum_values = Some(options);
        }
        spec
    }
}

fn attribute_spec(id: &str, title: &str, value_type: ValueType) -> FieldSpec {
    let mut spec = FieldSpec::fixed(
        ImportType::AssetRecords,
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

/// A field's title: the schema's, else the catalogue's for a built-in, else
/// the key in words.
fn field_title(rtype: &AssetRecordType, key: &str, prop: &Value) -> String {
    prop.get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .or_else(|| {
            rtype
                .is_builtin
                .then(|| en(&format!("asset-record-field-{}-{key}", rtype.slug)))
                .flatten()
        })
        .unwrap_or_else(|| humanize(key))
}

/// Every type field a column can feed, from the live types in order, by
/// title.
fn field_defs(types: &[TypeEntry]) -> Vec<FieldDef> {
    let mut defs: Vec<FieldDef> = Vec::new();
    for entry in types.iter().filter(|t| t.rtype.archived_at.is_none()) {
        let rtype = &entry.rtype;
        let Some(properties) = rtype
            .field_schema
            .get("properties")
            .and_then(Value::as_object)
        else {
            continue;
        };
        let mut keys: Vec<&String> = rtype
            .field_order
            .iter()
            .flatten()
            .filter(|k| properties.contains_key(*k))
            .collect();
        let rest: Vec<&String> = properties.keys().filter(|k| !keys.contains(k)).collect();
        keys.extend(rest);
        for key in keys {
            let prop = &properties[key];
            let at = match defs.iter().position(|d| &d.key == key) {
                Some(at) => at,
                None => {
                    let id = if TEMPLATE_HEADERS.contains(&key.as_str()) {
                        format!("{ATTRIBUTE_PREFIX}{key}")
                    } else {
                        key.clone()
                    };
                    defs.push(FieldDef {
                        key: key.clone(),
                        id,
                        title: field_title(rtype, key, prop),
                        types: Vec::new(),
                    });
                    defs.len() - 1
                }
            };
            defs[at]
                .types
                .push((rtype.slug.clone(), entry.label.clone(), prop.clone()));
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

/// The template's header row for `record_type` (a slug, or `None` for the
/// shared columns only): the shared columns, then the type's fields in its
/// order. `None` when no type or catalogue entry has that slug.
pub fn template_headers(
    conn: &mut DbConnection,
    record_type: Option<&str>,
) -> Result<Option<Vec<String>>, diesel::result::Error> {
    let mut headers: Vec<String> = TEMPLATE_HEADERS.iter().map(ToString::to_string).collect();
    let Some(slug) = record_type else {
        return Ok(Some(headers));
    };
    let types = load_types(conn)?;
    let Some(entry) = types
        .iter()
        .find(|t| t.slug() == slug && t.rtype.archived_at.is_none())
    else {
        return Ok(None);
    };
    let defs = field_defs(&types);
    let properties = entry
        .rtype
        .field_schema
        .get("properties")
        .and_then(Value::as_object);
    let mut keys: Vec<String> = entry.rtype.field_order.iter().flatten().cloned().collect();
    if let Some(properties) = properties {
        keys.retain(|k| properties.contains_key(k));
        let rest: Vec<String> = properties
            .keys()
            .filter(|k| !keys.contains(k))
            .cloned()
            .collect();
        keys.extend(rest);
    }
    headers.extend(
        keys.iter()
            .filter_map(|k| defs.iter().find(|d| &d.key == k).map(|d| d.id.clone())),
    );
    Ok(Some(headers))
}

pub struct AssetRecordImporter;

impl Importer for AssetRecordImporter {
    fn template_headers(&self) -> &'static [&'static str] {
        TEMPLATE_HEADERS
    }

    fn fields(&self, conn: &mut DbConnection) -> Result<Vec<FieldSpec>, diesel::result::Error> {
        use FieldGroup::{Core, Links};
        use ValueType::{Date, Enum, Number, Text, User};
        let t = ImportType::AssetRecords;
        let types = load_types(conn)?;
        let live: Vec<&TypeEntry> = types
            .iter()
            .filter(|e| e.rtype.archived_at.is_none())
            .collect();
        let mut outcomes: Vec<String> = Vec::new();
        for entry in &live {
            for outcome in entry.rtype.outcome_list() {
                if !outcomes.contains(&outcome.key) {
                    outcomes.push(outcome.key);
                }
            }
        }
        let mut fields = vec![
            FieldSpec::fixed(t, "asset_tag", "Asset tag", Core, Text, false),
            FieldSpec::fixed(t, "serial_number", "Serial number", Core, Text, false),
            FieldSpec {
                enum_labels: Some(live.iter().map(|e| e.label.clone()).collect()),
                ..FieldSpec::fixed(t, "record_type", "Record type", Core, Enum, true)
                    .with_enum(live.iter().map(|e| e.slug().to_string()).collect())
            },
            FieldSpec::fixed(t, "occurred_on", "Date of work", Core, Date, true)
                .with_format("YYYY-MM-DD"),
            FieldSpec::fixed(t, "completed_on", "Completed on", Core, Date, false)
                .with_format("YYYY-MM-DD"),
            FieldSpec::fixed(t, "outcome", "Outcome", Core, Enum, false).with_enum(outcomes),
            FieldSpec::fixed(t, "summary", "Summary", Core, Text, false),
            FieldSpec::fixed(t, "notes", "Notes", Core, Text, false),
            FieldSpec::fixed(t, "vendor", "Vendor", Core, Text, false),
            FieldSpec::fixed(t, "reference", "Reference", Core, Text, false),
            FieldSpec::fixed(t, "cost", "Cost", Core, Number, false),
            FieldSpec::fixed(t, "currency", "Currency", Core, Enum, false).with_enum(
                money::current_currencies()
                    .map(|c| c.code.to_string())
                    .collect(),
            ),
            FieldSpec::fixed(t, "location", "Location then", Core, Text, false),
            FieldSpec::fixed(t, "source_ref", "Source reference", Core, Text, false),
            FieldSpec::fixed(t, "performed_by", "Performed by", Links, User, false),
            FieldSpec::fixed(t, "holder", "Held by then", Links, User, false),
            FieldSpec::fixed(t, "ticket", "Ticket", Links, Text, false),
            FieldSpec::fixed(
                t,
                "replacement_asset_tag",
                "Replacement asset tag",
                Links,
                Text,
                false,
            ),
        ];
        fields.extend(field_defs(&types).iter().map(FieldDef::field_spec));
        Ok(fields)
    }

    /// Other spellings, Snipe-IT's maintenance export among them.
    fn synonyms(&self) -> &'static [(&'static str, &'static str)] {
        &[
            ("tag", "asset_tag"),
            ("assettag", "asset_tag"),
            ("tagno", "asset_tag"),
            ("assetno", "asset_tag"),
            ("assetnumber", "asset_tag"),
            ("serial", "serial_number"),
            ("sn", "serial_number"),
            ("serialno", "serial_number"),
            ("type", "record_type"),
            ("recordtype", "record_type"),
            ("worktype", "record_type"),
            ("maintenancetype", "record_type"),
            ("assetmaintenancetype", "record_type"),
            ("date", "occurred_on"),
            ("startdate", "occurred_on"),
            ("dateofwork", "occurred_on"),
            ("workdate", "occurred_on"),
            ("servicedate", "occurred_on"),
            ("repairdate", "occurred_on"),
            ("completiondate", "completed_on"),
            ("completed", "completed_on"),
            ("datecompleted", "completed_on"),
            ("enddate", "completed_on"),
            ("result", "outcome"),
            ("title", "summary"),
            ("description", "notes"),
            ("comments", "notes"),
            ("comment", "notes"),
            ("note", "notes"),
            ("supplier", "vendor"),
            ("repairer", "vendor"),
            ("serviceprovider", "vendor"),
            ("technician", "performed_by"),
            ("performedby", "performed_by"),
            ("doneby", "performed_by"),
            ("ref", "reference"),
            ("rma", "reference"),
            ("workorder", "reference"),
            ("invoice", "reference"),
            ("invoicenumber", "reference"),
            ("price", "cost"),
            ("amount", "cost"),
            ("ticketnumber", "ticket"),
            ("ticketno", "ticket"),
            ("heldby", "holder"),
            ("assignedto", "holder"),
            ("user", "holder"),
            ("replacementtag", "replacement_asset_tag"),
            ("replacedby", "replacement_asset_tag"),
            ("recordid", "source_ref"),
            ("externalid", "source_ref"),
            ("sourceid", "source_ref"),
        ]
    }

    fn value_choices(
        &self,
        conn: &mut DbConnection,
        field: &FieldSpec,
    ) -> Result<Option<Vec<Choice>>, diesel::result::Error> {
        match field.id.as_str() {
            "record_type" => Ok(Some(
                load_types(conn)?
                    .into_iter()
                    .map(|entry| {
                        let mut choice = Choice::new(entry.slug(), entry.label.clone());
                        if entry.rtype.archived_at.is_some() {
                            choice.disabled = Some(ARCHIVED_TYPE.to_string());
                        }
                        choice
                    })
                    .collect(),
            )),
            // Per type: the context's spec names its type.
            "outcome" => {
                let Some(slug) = field.kinds.as_ref().and_then(|k| k.first()) else {
                    return Ok(values::enum_choices(field));
                };
                let types = load_types(conn)?;
                let Some(entry) = types.iter().find(|t| t.slug() == slug) else {
                    return Ok(Some(Vec::new()));
                };
                Ok(Some(
                    entry
                        .rtype
                        .outcome_list()
                        .into_iter()
                        .map(|o| {
                            let label = outcome_label(&entry.rtype, &o.key, o.label.as_deref());
                            Choice::new(o.key, label)
                        })
                        .collect(),
                ))
            }
            _ => Ok(values::enum_choices(field)),
        }
    }

    fn value_synonyms(&self, field: &str) -> &'static [(&'static str, &'static str)] {
        match field {
            "record_type" => &[
                ("fix", "repair"),
                ("repairs", "repair"),
                ("hardware support", "repair"),
                ("maintenance", "preventative_maintenance"),
                ("preventive maintenance", "preventative_maintenance"),
                ("pm", "preventative_maintenance"),
                ("service", "preventative_maintenance"),
                ("servicing", "preventative_maintenance"),
                ("pat test", "inspection"),
                ("test and tag", "inspection"),
                ("safety inspection", "inspection"),
                ("calibration", "inspection"),
                ("firmware", "firmware_update"),
                ("firmware upgrade", "firmware_update"),
                ("software update", "firmware_update"),
            ],
            "outcome" => &[
                ("fixed", "repaired"),
                ("ok", "passed"),
                ("pass", "passed"),
                ("fail", "failed"),
                ("done", "completed"),
                ("nff", "no_fault_found"),
                ("ber", "beyond_repair"),
            ],
            _ => &[],
        }
    }

    /// Outcomes and type fields are answered per type: each type's own,
    /// typed as it types them.
    fn value_contexts(
        &self,
        conn: &mut DbConnection,
        field: &FieldSpec,
    ) -> Result<Option<Vec<ValueContext>>, diesel::result::Error> {
        if field.id == "outcome" {
            let types = load_types(conn)?;
            return Ok(Some(
                types
                    .iter()
                    .filter(|t| t.rtype.archived_at.is_none())
                    .map(|entry| {
                        let mut spec = field.clone();
                        spec.enum_values = Some(
                            entry
                                .rtype
                                .outcome_list()
                                .into_iter()
                                .map(|o| o.key)
                                .collect(),
                        );
                        spec.kinds = Some(vec![entry.slug().to_string()]);
                        ValueContext {
                            key: entry.slug().to_string(),
                            label: entry.label.clone(),
                            spec,
                        }
                    })
                    .collect(),
            ));
        }
        if field.group != FieldGroup::Attribute {
            return Ok(None);
        }
        let types = load_types(conn)?;
        let Some(def) = field_defs(&types).into_iter().find(|d| d.id == field.id) else {
            return Ok(Some(Vec::new()));
        };
        Ok(Some(
            def.types
                .iter()
                .map(|(slug, label, prop)| {
                    let mut spec = attribute_spec(&def.id, &field.label, value_type_of(prop));
                    if spec.value_type == ValueType::Enum {
                        spec.enum_values = Some(enum_options(prop));
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

    fn context_family(&self, field: &FieldSpec) -> Option<&'static str> {
        (field.id == "outcome" || field.group == FieldGroup::Attribute).then_some(TYPE_FAMILY)
    }

    fn context_families(&self) -> &'static [&'static str] {
        &[TYPE_FAMILY]
    }

    fn lookups(
        &self,
        conn: &mut DbConnection,
        field: &FieldSpec,
        _contexts: &[String],
    ) -> Result<Option<Vec<values::LookupContext>>, diesel::result::Error> {
        if !matches!(field.id.as_str(), "performed_by" | "holder") {
            return Ok(None);
        }
        Ok(Some(vec![values::LookupContext {
            key: String::new(),
            label: None,
            lookup: values::Lookup::new(LookupKind::Person, member_choices(conn)?),
        }]))
    }

    /// Each row's type, as [`plan_row`] reads it: its cell, else the type
    /// of the record its source reference updates.
    fn row_contexts(
        &self,
        conn: &mut DbConnection,
        grid: &ParsedCsv,
        options: &ImportOptions,
        family: &str,
    ) -> Result<HashMap<usize, String>, diesel::result::Error> {
        let mut out = HashMap::new();
        if family != TYPE_FAMILY {
            return Ok(out);
        }
        let types = load_types(conn)?;
        let refs: Vec<String> = grid
            .records()
            .map(|r| cell(&r.values, "source_ref"))
            .filter(|r| !r.is_empty())
            .collect();
        let existing = existing_by_ref(conn, &refs)?;
        for record in grid.records() {
            let row = with_constants(record.values, &existing, options);
            let slug = match find_type(&types, &cell(&row, "record_type")) {
                Some(entry) => Some(entry.slug().to_string()),
                None if cell(&row, "record_type").is_empty() => existing
                    .get(&cell(&row, "source_ref"))
                    .and_then(|r| types.iter().find(|t| t.rtype.id == r.record_type_id))
                    .map(|t| t.slug().to_string()),
                None => None,
            };
            if let Some(slug) = slug {
                out.insert(record.source_row, slug);
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
        let lookups = Lookups::load(conn, parsed)?;
        let planned = plan_all(conn, parsed, &lookups, &ctx.options)?;
        let mut plan = RowPlan {
            notices: planned.notices,
            catalog: planned.catalog,
            history: Some(planned.facts),
            ..Default::default()
        };
        for entry in planned.entries {
            plan.rows.push(entry.result());
        }
        Ok(plan)
    }

    /// Adds the catalogue types the rows use, then writes exactly the rows
    /// the dry run counted: the same plan runs again against the types as
    /// added, and invalid rows are skipped.
    fn commit(
        &self,
        conn: &mut DbConnection,
        parsed: &ParsedCsv,
        ctx: &ImportContext,
    ) -> Result<Committed, CommitError> {
        let lookups = Lookups::load(conn, parsed)?;
        let first = plan_all(conn, parsed, &lookups, &ctx.options)?;
        let lookups = if first.catalog.record_types.is_empty() {
            lookups
        } else {
            for slug in &first.catalog.record_types {
                type_repo::add_from_catalogue(conn, slug, ctx.actor).map_err(|e| {
                    CommitError::Db(diesel::result::Error::QueryBuilderError(
                        format!("adding record type {slug}: {e:?}").into(),
                    ))
                })?;
            }
            Lookups::load(conn, parsed)?
        };
        let planned = plan_all(conn, parsed, &lookups, &ctx.options)?;

        let mut records = Vec::new();
        let mut rows = Vec::with_capacity(planned.entries.len());
        for entry in planned.entries {
            let row_num = entry.source_row;
            let written = match &entry.planned {
                Ok(Planned {
                    action: Action::Create(input),
                    ..
                }) => {
                    let mut input = input.clone();
                    input.import_job_id = Some(ctx.job_id);
                    Some(record_repo::create(conn, input).map_err(|e| write_error(row_num, e))?)
                }
                Ok(Planned {
                    action:
                        Action::Edit {
                            id,
                            version,
                            changes,
                        },
                    ..
                }) => Some(
                    record_repo::update(conn, *id, *version, changes.clone(), None)
                        .map_err(|e| write_error(row_num, e))?,
                ),
                _ => None,
            };
            let mut result = entry.result();
            if let Some(written) = written {
                result.record_id = Some(RecordId::Int(written.record.id));
                result.observed_updated_at = Some(observed(written.record.updated_at));
                records.push(written.record);
            }
            rows.push(result);
        }
        Ok(Committed {
            records: ImportedRecords::AssetRecords(records),
            rows,
            created: Vec::new(),
        })
    }
}

/// The longest source reference a record holds.
const SOURCE_REF_MAX: usize = 255;

/// Why an archived type can't be picked for new records.
const ARCHIVED_TYPE: &str = "Archived; restore it under Settings to import records of it";

/// A record's `updated_at` as a row result keeps it.
fn observed(at: DateTime<Utc>) -> String {
    at.naive_utc().format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
}

/// A cell, trimmed.
fn cell(row: &HashMap<String, String>, key: &str) -> String {
    row.get(key)
        .map(|v| v.trim().to_string())
        .unwrap_or_default()
}

fn opt_cell(row: &HashMap<String, String>, key: &str) -> Option<String> {
    Some(cell(row, key)).filter(|v| !v.is_empty())
}

/// The records carrying any of `refs` as their source reference.
fn existing_by_ref(
    conn: &mut DbConnection,
    refs: &[String],
) -> Result<HashMap<String, AssetRecord>, diesel::result::Error> {
    use crate::schema::asset_records;
    if refs.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<AssetRecord> = asset_records::table
        .filter(asset_records::source_ref.eq_any(refs))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.source_ref.clone().map(|k| (k, r)))
        .collect())
}

/// The row with the job's constants as the record takes them: a row that
/// updates an existing record drops the ones only new records take.
fn with_constants(
    mut row: HashMap<String, String>,
    existing: &HashMap<String, AssetRecord>,
    options: &ImportOptions,
) -> HashMap<String, String> {
    if !options.constants.is_empty() && existing.contains_key(&cell(&row, "source_ref")) {
        options.drop_new_only_constants(&mut row);
    }
    row
}

/// What rows are read against.
struct Lookups {
    tz: Tz,
    types: Vec<TypeEntry>,
    fields: Vec<FieldDef>,
    assets: HashMap<i32, Asset>,
    /// Every asset holding each tag, and each serial number, as written
    /// and lowercased.
    tags: HashMap<String, Vec<i32>>,
    tags_lower: HashMap<String, Vec<i32>>,
    serials: HashMap<String, Vec<i32>>,
    serials_lower: HashMap<String, Vec<i32>>,
    /// Records the file's source references name.
    by_ref: HashMap<String, AssetRecord>,
    /// Members by lowercased primary email, and their emails.
    members: HashMap<String, Uuid>,
    member_emails: HashMap<Uuid, String>,
    /// The members who can work tickets here: who may be named as having
    /// done the work.
    performers: HashSet<Uuid>,
    /// The tickets the file's ticket numbers name, by number.
    tickets: HashMap<i32, i32>,
    /// Ticket numbers by id, for the tickets records being updated link.
    ticket_numbers: HashMap<i32, i32>,
    /// The workspace currency, or the one its locale suggests.
    currency: Option<String>,
}

impl Lookups {
    fn load(conn: &mut DbConnection, parsed: &ParsedCsv) -> Result<Self, diesel::result::Error> {
        use crate::schema::{assets, tickets};
        let tz = record_repo::workspace_timezone(conn)?;
        let types = load_types(conn)?;
        let fields = field_defs(&types);
        let rows: Vec<Asset> = assets::table
            .filter(
                assets::asset_tag
                    .is_not_null()
                    .or(assets::serial_number.is_not_null()),
            )
            .load(conn)?;
        let mut tags: HashMap<String, Vec<i32>> = HashMap::new();
        let mut tags_lower: HashMap<String, Vec<i32>> = HashMap::new();
        let mut serials: HashMap<String, Vec<i32>> = HashMap::new();
        let mut serials_lower: HashMap<String, Vec<i32>> = HashMap::new();
        let mut by_id = HashMap::new();
        for asset in rows {
            if let Some(tag) = asset
                .asset_tag
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                tags.entry(tag.to_string()).or_default().push(asset.id);
                tags_lower
                    .entry(tag.to_lowercase())
                    .or_default()
                    .push(asset.id);
            }
            if let Some(serial) = asset
                .serial_number
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                serials
                    .entry(serial.to_string())
                    .or_default()
                    .push(asset.id);
                serials_lower
                    .entry(serial.to_lowercase())
                    .or_default()
                    .push(asset.id);
            }
            by_id.insert(asset.id, asset);
        }

        let refs: Vec<String> = parsed
            .records()
            .map(|r| cell(&r.values, "source_ref"))
            .filter(|r| !r.is_empty())
            .collect();
        let by_ref = existing_by_ref(conn, &refs)?;
        // The assets the records being updated are on, tag or not.
        let missing: Vec<i32> = by_ref
            .values()
            .map(|r| r.asset_id)
            .filter(|id| !by_id.contains_key(id))
            .collect();
        if !missing.is_empty() {
            let more: Vec<Asset> = assets::table
                .filter(assets::id.eq_any(&missing))
                .load(conn)?;
            for asset in more {
                by_id.insert(asset.id, asset);
            }
        }

        let mut members = HashMap::new();
        let mut member_emails = HashMap::new();
        for (user, email) in workspace_repo::member_primary_emails(conn)? {
            members.insert(email.to_lowercase(), user);
            member_emails.insert(user, email);
        }

        let people: Vec<Uuid> = member_emails.keys().copied().collect();
        let performers = crate::repository::assignees::assignable_among(conn, &people)?;

        let numbers: Vec<i32> = parsed
            .records()
            .filter_map(|r| ticket_number(&cell(&r.values, "ticket")))
            .collect();
        let found: Vec<(i32, i32)> = if numbers.is_empty() {
            Vec::new()
        } else {
            tickets::table
                .filter(tickets::number.eq_any(&numbers))
                .select((tickets::number, tickets::id))
                .load(conn)?
        };
        let linked: Vec<i32> = by_ref.values().filter_map(|r| r.ticket_id).collect();
        let mut ticket_numbers = load_ticket_numbers(conn, &linked)?;
        ticket_numbers.extend(found.iter().map(|(number, id)| (*id, *number)));

        let settings = crate::repository::site_settings::find_site_settings(conn)?;
        let currency = settings.and_then(|s| {
            s.currency_code
                .clone()
                .or_else(|| money::suggested_currency(&s.default_locale).map(String::from))
        });

        Ok(Self {
            tz,
            types,
            fields,
            assets: by_id,
            tags,
            tags_lower,
            serials,
            serials_lower,
            by_ref,
            members,
            member_emails,
            performers,
            tickets: found.into_iter().collect(),
            ticket_numbers,
            currency,
        })
    }

    fn type_by_id(&self, id: i32) -> Option<&TypeEntry> {
        self.types.iter().find(|t| t.rtype.id == id)
    }

    /// The member a cell names by primary email (any case) or user id.
    fn member(&self, raw: &str) -> Option<Uuid> {
        if raw.contains('@') {
            return self.members.get(&raw.to_lowercase()).copied();
        }
        let id = Uuid::parse_str(raw).ok()?;
        self.member_emails.contains_key(&id).then_some(id)
    }

    /// A member as a cell names them: their primary email.
    fn member_text(&self, user: Uuid) -> String {
        self.member_emails
            .get(&user)
            .cloned()
            .unwrap_or_else(|| user.to_string())
    }

    fn asset_tag_of(&self, id: i32) -> Option<String> {
        self.assets.get(&id).and_then(|a| a.asset_tag.clone())
    }

    /// The field id of type field `key`.
    fn field_id(&self, key: &str) -> String {
        self.fields
            .iter()
            .find(|d| d.key == key)
            .map_or_else(|| key.to_string(), |d| d.id.clone())
    }

    fn kind_label(&self, kind: &str) -> String {
        humanize(kind)
    }
}

/// The numbers people quote of the tickets of `ids`, by id.
fn load_ticket_numbers(
    conn: &mut DbConnection,
    ids: &[i32],
) -> Result<HashMap<i32, i32>, diesel::result::Error> {
    use crate::schema::tickets;
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(i32, i32)> = tickets::table
        .filter(tickets::id.eq_any(ids))
        .select((tickets::id, tickets::number))
        .load(conn)?;
    Ok(rows.into_iter().collect())
}

/// The ticket a cell names by its number as people quote it: `412`,
/// `#412`.
fn ticket_number(raw: &str) -> Option<i32> {
    let digits = raw.trim().trim_start_matches('#').trim();
    digits.parse::<i32>().ok().filter(|n| *n > 0)
}

/// Assets holding `value` in `exact`, else in `lower` when the case differs.
fn holders<'a>(
    exact: &'a HashMap<String, Vec<i32>>,
    lower: &'a HashMap<String, Vec<i32>>,
    value: &str,
) -> &'a [i32] {
    exact
        .get(value)
        .or_else(|| lower.get(&value.to_lowercase()))
        .map_or(&[], Vec::as_slice)
}

/// Why a row found no asset, or found too many.
struct NoAsset {
    column: Option<String>,
    message: String,
    group: Option<String>,
}

impl NoAsset {
    fn new(column: Option<&str>, message: String, group: Option<&str>) -> Self {
        Self {
            column: column.map(String::from),
            message,
            group: group.map(String::from),
        }
    }

    /// Whether the row's asset isn't there at all, rather than unclear.
    fn unmatched(&self) -> bool {
        self.group.is_some()
    }
}

fn shared(lookups: &Lookups, what: &str, value: &str, ids: &[i32]) -> String {
    let names: Vec<String> = ids
        .iter()
        .take(3)
        .filter_map(|id| lookups.assets.get(id).map(|a| format!("'{}'", a.name)))
        .collect();
    format!(
        "{what} '{value}' is held by {} assets ({}); give each its own before importing their history",
        ids.len(),
        names.join(", ")
    )
}

/// The one asset the row's tag or serial names, by `by`.
fn match_asset(
    row: &HashMap<String, String>,
    lookups: &Lookups,
    by: MatchBy,
) -> Result<i32, NoAsset> {
    let tag = cell(row, "asset_tag");
    let serial = cell(row, "serial_number");
    let by_tag = || match holders(&lookups.tags, &lookups.tags_lower, &tag) {
        [id] => Ok(Some(*id)),
        [] => Ok(None),
        ids => Err(NoAsset::new(
            Some("asset_tag"),
            shared(lookups, "tag", &tag, ids),
            None,
        )),
    };
    let by_serial = || match holders(&lookups.serials, &lookups.serials_lower, &serial) {
        [id] => Ok(Some(*id)),
        [] => Ok(None),
        ids => Err(NoAsset::new(
            Some("serial_number"),
            shared(lookups, "serial number", &serial, ids),
            None,
        )),
    };
    match by {
        MatchBy::AssetTag | MatchBy::None => {
            if tag.is_empty() {
                return Err(NoAsset::new(
                    Some("asset_tag"),
                    NO_TAG_TO_MATCH.to_string(),
                    None,
                ));
            }
            by_tag()?.ok_or_else(|| {
                NoAsset::new(
                    Some("asset_tag"),
                    format!("{NO_ASSET_WITH_TAG}'{tag}'"),
                    Some(NO_ASSET_WITH_THIS_TAG),
                )
            })
        }
        MatchBy::SerialNumber => {
            if serial.is_empty() {
                return Err(NoAsset::new(
                    Some("serial_number"),
                    NO_SERIAL_TO_MATCH.to_string(),
                    None,
                ));
            }
            by_serial()?.ok_or_else(|| {
                NoAsset::new(
                    Some("serial_number"),
                    format!("{NO_ASSET_WITH_SERIAL}'{serial}'"),
                    Some(NO_ASSET_WITH_THIS_SERIAL),
                )
            })
        }
        MatchBy::TagThenSerial => {
            if tag.is_empty() && serial.is_empty() {
                return Err(NoAsset::new(
                    None,
                    "no asset tag or serial number to match an asset by".to_string(),
                    None,
                ));
            }
            let from_tag = if tag.is_empty() { None } else { by_tag()? };
            let from_serial = if serial.is_empty() {
                None
            } else {
                by_serial()?
            };
            match (from_tag, from_serial) {
                (Some(a), Some(b)) if a != b => Err(NoAsset::new(
                    None,
                    format!("tag '{tag}' and serial number '{serial}' name different assets"),
                    None,
                )),
                (Some(id), _) | (None, Some(id)) => Ok(id),
                (None, None) => {
                    let message = match (tag.is_empty(), serial.is_empty()) {
                        (false, false) => {
                            format!("no asset with tag '{tag}' or serial number '{serial}'")
                        }
                        (false, true) => format!("{NO_ASSET_WITH_TAG}'{tag}'"),
                        _ => format!("{NO_ASSET_WITH_SERIAL}'{serial}'"),
                    };
                    Err(NoAsset::new(
                        None,
                        message,
                        Some(NO_ASSET_WITH_THIS_TAG_OR_SERIAL),
                    ))
                }
            }
        }
    }
}

/// A time of day as sheets write it.
fn parse_time(raw: &str) -> Option<NaiveTime> {
    let raw = raw.trim();
    [
        "%H:%M:%S%.f",
        "%H:%M:%S",
        "%H:%M",
        "%I:%M %p",
        "%I:%M:%S %p",
        "%I:%M%p",
    ]
    .iter()
    .find_map(|f| NaiveTime::parse_from_str(&raw.to_uppercase(), f).ok())
}

/// A date cell as the day of the work, or the moment when it has a time. A
/// time without an offset is read in the workspace's timezone by the shared
/// wall-clock rule ([`local_moment`]: a time a clock change skips is the
/// first moment after it, a repeated one the first of the two); exactly
/// midnight is a date, as a workbook writes one.
fn read_work_date(raw: &str, order: DateOrder, tz: Tz) -> Option<WorkDate> {
    let raw = raw.trim();
    if let Ok(at) = DateTime::parse_from_rfc3339(raw) {
        return Some(WorkDate::At(at.with_timezone(&Utc)));
    }
    if let Some((date, time)) = raw.split_once(['T', ' ']) {
        let day = values::parse_date(date, order)?;
        let time = parse_time(time)?;
        if time == NaiveTime::MIN {
            return Some(WorkDate::Day(day));
        }
        return Some(WorkDate::At(local_moment(
            NaiveDateTime::new(day, time),
            tz,
        )));
    }
    values::parse_date(raw, order).map(WorkDate::Day)
}

/// A day or moment as a cell writes it back: the day, or the moment in the
/// workspace's timezone with its offset, to the second as stored, so it
/// reads back as the same moment even in an hour a clock change repeats.
fn work_date_text(on: NaiveDate, at: Option<DateTime<Utc>>, tz: Tz) -> String {
    match at {
        Some(at) => at
            .with_timezone(&tz)
            .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false),
        None => on.format("%Y-%m-%d").to_string(),
    }
}

/// The day a date falls on in the workspace.
fn day_of(date: WorkDate, tz: Tz) -> NaiveDate {
    match date {
        WorkDate::Day(day) => day,
        WorkDate::At(at) => at.with_timezone(&tz).date_naive(),
    }
}

/// What the commit does with a row that checks out.
#[derive(Debug, Clone)]
enum Action {
    Create(NewRecord),
    Edit {
        id: i32,
        version: i32,
        changes: RecordChanges,
    },
    /// The record the row's reference names already holds every value.
    Unchanged(i32),
    /// A record already has this asset, type, day and reference.
    LooksRecorded(i32),
}

#[derive(Debug, Clone)]
struct Planned {
    action: Action,
    asset_id: i32,
    type_slug: String,
    occurred_on: NaiveDate,
    /// For a new record's duplicate check: the reference, else the cost,
    /// else the summary, as compared.
    likeness: Likeness,
    cost: Option<(String, i64)>,
    fields_written: BTreeMap<String, FieldChange>,
    dropped: Vec<DroppedCell>,
    warnings: Vec<RecordWarning>,
}

struct Entry {
    source_row: usize,
    key: Option<String>,
    label: String,
    planned: Result<Planned, Vec<CellError>>,
}

impl Entry {
    fn result(&self) -> RowResult {
        let mut result = match &self.planned {
            Ok(p) => {
                let (outcome, id) = match &p.action {
                    Action::Create(_) => (RowOutcome::Create, None),
                    Action::Edit { id, .. } => (RowOutcome::Update, Some(*id)),
                    Action::Unchanged(id) | Action::LooksRecorded(id) => {
                        (RowOutcome::Unchanged, Some(*id))
                    }
                };
                let mut r = RowResult::planned(self.source_row, outcome);
                r.record_id = id.map(RecordId::Int);
                r.kind = Some(p.type_slug.clone());
                r.dropped = p.dropped.clone();
                r.fields_written = p.fields_written.clone();
                r
            }
            Err(errors) => {
                let mut r = RowResult::planned(self.source_row, RowOutcome::Skipped);
                r.errors = errors.clone();
                r
            }
        };
        result = result
            .with_key(self.key.clone())
            .with_label(self.label.clone());
        result
    }
}

struct PlannedFile {
    entries: Vec<Entry>,
    notices: Vec<Notice>,
    catalog: CatalogPlan,
    facts: HistoryFacts,
}

fn error(column: Option<&str>, message: impl Into<String>) -> CellError {
    CellError {
        column: column.map(String::from),
        source_column: None,
        message: message.into(),
        group: None,
    }
}

/// Plan every row, in file order, then check the new ones against the
/// records already there.
fn plan_all(
    conn: &mut DbConnection,
    parsed: &ParsedCsv,
    lookups: &Lookups,
    options: &ImportOptions,
) -> Result<PlannedFile, diesel::result::Error> {
    // Source references more than one row holds: each such row is skipped.
    let mut ref_rows: HashMap<String, Vec<usize>> = HashMap::new();
    for record in parsed.records() {
        let r = cell(&record.values, "source_ref");
        if !r.is_empty() {
            ref_rows.entry(r).or_default().push(record.source_row);
        }
    }
    ref_rows.retain(|_, rows| rows.len() > 1);

    let mut entries = Vec::with_capacity(parsed.rows.len());
    let mut unmatched = 0;
    for record in parsed.records() {
        let row = with_constants(record.values, &lookups.by_ref, options);
        let source_ref = cell(&row, "source_ref");
        let key = Some(source_ref.clone())
            .filter(|r| !r.is_empty())
            .or_else(|| opt_cell(&row, "asset_tag"))
            .or_else(|| opt_cell(&row, "serial_number"));
        let planned = match ref_rows.get(&source_ref) {
            Some(rows) => Err(vec![error(
                Some("source_ref"),
                duplicate_in_file("source reference", &source_ref, rows),
            )]),
            None => plan_row(conn, &row, lookups, options)?,
        };
        if let Err(errors) = &planned {
            if errors.iter().any(|e| e.group.is_some()) {
                unmatched += 1;
            }
        }
        let label = row_label(&row, &planned, lookups);
        entries.push(Entry {
            source_row: record.source_row,
            key,
            label,
            planned,
        });
    }

    let duplicates = if options.import_likely_duplicates {
        0
    } else {
        mark_likely_duplicates(conn, &mut entries)?
    };

    let mut notices = Vec::new();
    let mut catalog = CatalogPlan::default();
    let mut facts = HistoryFacts::default();
    let mut assets: HashSet<i32> = HashSet::new();
    let mut costs: BTreeMap<String, (i128, usize)> = BTreeMap::new();
    let (mut before_purchase, mut long_after) = (0, 0);
    for entry in &entries {
        let Ok(p) = &entry.planned else { continue };
        if !matches!(p.action, Action::Create(_) | Action::Edit { .. }) {
            continue;
        }
        assets.insert(p.asset_id);
        facts.earliest = Some(
            facts
                .earliest
                .map_or(p.occurred_on, |d| d.min(p.occurred_on)),
        );
        facts.latest = Some(facts.latest.map_or(p.occurred_on, |d| d.max(p.occurred_on)));
        if let Some((code, minor)) = &p.cost {
            let total = costs.entry(code.clone()).or_default();
            total.0 += i128::from(*minor);
            total.1 += 1;
        }
        for warning in &p.warnings {
            match warning {
                RecordWarning::BeforePurchase { .. } => before_purchase += 1,
                RecordWarning::CompletedLongAfter => long_after += 1,
                // Nearly all history predates what the asset can tell, so
                // an unknown holder isn't worth a line.
                RecordWarning::HolderUnknown => {}
            }
        }
        if let Some(entry) = lookups.types.iter().find(|t| t.slug() == p.type_slug) {
            if entry.from_catalogue && !catalog.record_types.contains(&p.type_slug) {
                catalog.record_types.push(p.type_slug.clone());
            }
        }
    }
    facts.assets = assets.len();
    facts.costs = costs
        .into_iter()
        .map(|(currency, (minor, rows))| CostTotal {
            amount: i64::try_from(minor)
                .ok()
                .and_then(|m| money::format_minor(m, &currency).ok())
                .unwrap_or_default(),
            currency,
            rows,
        })
        .collect();

    let counted = |code: &str, count: usize, one: &str, many: String| Notice {
        code: code.to_string(),
        count,
        message: if count == 1 { one.to_string() } else { many },
    };
    if unmatched > 0 {
        notices.push(counted(
            UNMATCHED_ASSETS,
            unmatched,
            "1 row is about an asset that isn't here. Import the assets first, then this file.",
            format!(
                "{unmatched} rows are about assets that aren't here. Import the assets first, then this file."
            ),
        ));
    }
    if duplicates > 0 {
        notices.push(counted(
            LIKELY_DUPLICATE,
            duplicates,
            "1 row looks already recorded and is skipped.",
            format!("{duplicates} rows look already recorded and are skipped."),
        ));
    }
    if before_purchase > 0 {
        notices.push(counted(
            BEFORE_PURCHASE,
            before_purchase,
            "1 record is dated before its asset's purchase date.",
            format!("{before_purchase} records are dated before their asset's purchase date."),
        ));
    }
    if long_after > 0 {
        notices.push(counted(
            COMPLETED_LONG_AFTER,
            long_after,
            "1 record was completed more than five years after the work started.",
            format!(
                "{long_after} records were completed more than five years after the work started."
            ),
        ));
    }
    Ok(PlannedFile {
        entries,
        notices,
        catalog,
        facts,
    })
}

/// What a row is called: its asset's name and the summary, else the type.
fn row_label(
    row: &HashMap<String, String>,
    planned: &Result<Planned, Vec<CellError>>,
    lookups: &Lookups,
) -> String {
    let what = opt_cell(row, "summary").or_else(|| {
        planned.as_ref().ok().and_then(|p| {
            lookups
                .types
                .iter()
                .find(|t| t.slug() == p.type_slug)
                .map(|t| t.label.clone())
        })
    });
    let asset = planned
        .as_ref()
        .ok()
        .and_then(|p| lookups.assets.get(&p.asset_id))
        .map(|a| a.name.clone());
    match (asset, what) {
        (Some(asset), Some(what)) => format!("{asset}: {what}"),
        (Some(one), None) | (None, Some(one)) => one,
        (None, None) => String::new(),
    }
}

/// Plan new records without a source reference that look already
/// recorded: one of the asset's records has the same type and day, and
/// the same reference, else the same cost, else the same summary. How many.
fn mark_likely_duplicates(
    conn: &mut DbConnection,
    entries: &mut [Entry],
) -> Result<usize, diesel::result::Error> {
    use crate::schema::{asset_record_types, asset_records};
    let assets: Vec<i32> = entries
        .iter()
        .filter_map(|e| match &e.planned {
            Ok(p) => match &p.action {
                Action::Create(input) if input.source_ref.is_none() => Some(p.asset_id),
                _ => None,
            },
            Err(_) => None,
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if assets.is_empty() {
        return Ok(0);
    }
    let slugs: HashMap<i32, String> = asset_record_types::table
        .select((asset_record_types::id, asset_record_types::slug))
        .load::<(i32, String)>(conn)?
        .into_iter()
        .collect();
    type Stored = (
        i32,
        i32,
        i32,
        NaiveDate,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
    );
    let stored: Vec<Stored> = asset_records::table
        .filter(asset_records::asset_id.eq_any(&assets))
        .select((
            asset_records::id,
            asset_records::asset_id,
            asset_records::record_type_id,
            asset_records::occurred_on,
            asset_records::reference,
            asset_records::cost_minor,
            asset_records::cost_currency,
            asset_records::summary,
        ))
        .order(asset_records::id.asc())
        .load(conn)?;
    let mut index: HashMap<(i32, String, NaiveDate), Vec<(i32, Likeness)>> = HashMap::new();
    for (id, asset, type_id, on, reference, minor, currency, summary) in stored {
        let Some(slug) = slugs.get(&type_id).cloned() else {
            continue;
        };
        let likeness = Likeness::of(
            reference.as_deref(),
            minor.zip(currency),
            summary.as_deref(),
        );
        index
            .entry((asset, slug, on))
            .or_default()
            .push((id, likeness));
    }
    let mut count = 0;
    for entry in entries.iter_mut() {
        let Ok(p) = &mut entry.planned else { continue };
        let Action::Create(input) = &p.action else {
            continue;
        };
        if input.source_ref.is_some() {
            continue;
        }
        let key = (p.asset_id, p.type_slug.clone(), p.occurred_on);
        let found = index.get(&key).and_then(|stored| {
            stored
                .iter()
                .find(|(_, other)| p.likeness.same_work(other))
                .map(|(id, _)| *id)
        });
        if let Some(id) = found {
            p.action = Action::LooksRecorded(id);
            count += 1;
        }
    }
    Ok(count)
}

/// What tells a record from another of the same asset, type and day, each
/// as compared (text trimmed, case ignored).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Likeness {
    reference: Option<String>,
    cost: Option<(i64, String)>,
    summary: Option<String>,
}

impl Likeness {
    fn of(reference: Option<&str>, cost: Option<(i64, String)>, summary: Option<&str>) -> Self {
        let text = |v: Option<&str>| v.map(normalize_value).filter(|v| !v.is_empty());
        Self {
            reference: text(reference),
            cost,
            summary: text(summary),
        }
    }

    /// Whether two records look like the same work: the same reference when
    /// both have one, else the same cost when both have one, else the same
    /// summary (blank matching blank). A sheet without a reference column
    /// still finds the history another sheet imported with one.
    fn same_work(&self, other: &Self) -> bool {
        if let (Some(a), Some(b)) = (&self.reference, &other.reference) {
            return a == b;
        }
        if let (Some(a), Some(b)) = (&self.cost, &other.cost) {
            return a == b;
        }
        self.summary == other.summary
    }
}

/// The record type a row names.
enum TypePick<'a> {
    Named(&'a TypeEntry),
    Blank,
}

/// Check one row and say what the commit does with it.
fn plan_row(
    conn: &mut DbConnection,
    row: &HashMap<String, String>,
    lookups: &Lookups,
    options: &ImportOptions,
) -> Result<Result<Planned, Vec<CellError>>, diesel::result::Error> {
    let mut errors: Vec<CellError> = Vec::new();
    let mut dropped: Vec<DroppedCell> = Vec::new();
    let mode = options.row_mode();
    let tz = lookups.tz;
    let order = options.date_order;
    let mark = options.decimal_mark();

    // The record a source reference names, under the mode.
    let source_ref = opt_cell(row, "source_ref");
    if source_ref
        .as_ref()
        .is_some_and(|r| r.chars().count() > SOURCE_REF_MAX)
    {
        return Ok(Err(vec![error(
            Some("source_ref"),
            format!("longer than {SOURCE_REF_MAX} characters"),
        )]));
    }
    let existing = source_ref.as_ref().and_then(|r| lookups.by_ref.get(r));
    match (existing, mode) {
        (Some(_), ImportMode::CreateOnly) => errors.push(error(
            Some("source_ref"),
            format!(
                "a record with source reference '{}' already exists; only new records are imported",
                source_ref.as_deref().unwrap_or_default()
            ),
        )),
        (None, ImportMode::UpdateOnly) => errors.push(error(
            Some("source_ref"),
            match &source_ref {
                Some(r) => format!("no record has source reference '{r}'"),
                None => "no source reference to match a record by".to_string(),
            },
        )),
        _ => {}
    }
    if !errors.is_empty() {
        return Ok(Err(errors));
    }

    // The asset: the one the row's key names; on an update, the record's
    // own, which the key may confirm but not change.
    let has_key = !cell(row, "asset_tag").is_empty() || !cell(row, "serial_number").is_empty();
    let asset_id = match existing {
        Some(record) => {
            if has_key {
                match match_asset(row, lookups, options.key()) {
                    Ok(id) if id != record.asset_id => {
                        let name = lookups
                            .assets
                            .get(&record.asset_id)
                            .map_or_else(String::new, |a| a.name.clone());
                        errors.push(error(
                            None,
                            format!(
                                "record '{}' is on '{name}', not the asset this row names; a record can't move to another asset",
                                source_ref.as_deref().unwrap_or_default()
                            ),
                        ));
                    }
                    _ => {}
                }
            }
            record.asset_id
        }
        None => match match_asset(row, lookups, options.key()) {
            Ok(id) => id,
            Err(no) => {
                let unmatched = no.unmatched();
                errors.push(CellError {
                    column: no.column,
                    source_column: None,
                    message: no.message,
                    group: no.group,
                });
                if unmatched {
                    // The rest of the row can't be checked without its asset.
                    return Ok(Err(errors));
                }
                0
            }
        },
    };
    let Some(asset) = lookups.assets.get(&asset_id) else {
        return Ok(Err(errors));
    };

    // The type: the cell's, else the record's.
    let type_cell = cell(row, "record_type");
    let pick = if type_cell.is_empty() {
        TypePick::Blank
    } else {
        match find_type(&lookups.types, &type_cell) {
            Some(entry) => TypePick::Named(entry),
            None => {
                errors.push(error(
                    Some("record_type"),
                    format!(
                        "unknown record type '{type_cell}'. Use one of: {}, or add it under Settings, Assets, Record types",
                        lookups
                            .types
                            .iter()
                            .filter(|t| t.rtype.archived_at.is_none())
                            .map(|t| t.label.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ));
                return Ok(Err(errors));
            }
        }
    };
    let entry = match (&pick, existing) {
        (TypePick::Named(entry), _) => *entry,
        (TypePick::Blank, Some(record)) => match lookups.type_by_id(record.record_type_id) {
            Some(entry) => entry,
            None => {
                return Ok(Err(vec![error(
                    Some("record_type"),
                    "record type not found",
                )]))
            }
        },
        (TypePick::Blank, None) => {
            errors.push(error(Some("record_type"), "record type is required"));
            return Ok(Err(errors));
        }
    };
    let rtype = &entry.rtype;
    let changes_type = existing.is_none_or(|r| r.record_type_id != rtype.id);
    if changes_type && rtype.archived_at.is_some() {
        errors.push(error(
            Some("record_type"),
            format!(
                "{} is archived; restore it under Settings to import records of it",
                entry.label
            ),
        ));
    }

    // Dates.
    let read_date = |field: &str, errors: &mut Vec<CellError>| -> Option<WorkDate> {
        let raw = cell(row, field);
        if raw.is_empty() {
            return None;
        }
        let Some(date) = read_work_date(&raw, order, tz) else {
            errors.push(error(
                Some(field),
                format!(
                    "'{raw}' is not a valid date; use {}, with a time after it if you know one",
                    values::date_hint(order)
                ),
            ));
            return None;
        };
        let day = day_of(date, tz);
        let today = Utc::now().with_timezone(&tz).date_naive();
        if day < EARLIEST_DAY {
            errors.push(error(Some(field), "dates start at 1900-01-01"));
            return None;
        }
        if day > today.succ_opt().unwrap_or(today) {
            errors.push(error(
                Some(field),
                format!("'{raw}' is in the future; work can't be dated after tomorrow"),
            ));
            return None;
        }
        Some(date)
    };
    let occurred = read_date("occurred_on", &mut errors);
    let completed = read_date("completed_on", &mut errors);
    if existing.is_none() && occurred.is_none() && cell(row, "occurred_on").is_empty() {
        errors.push(error(Some("occurred_on"), "date of work is required"));
    }

    // Outcome, one of the type's.
    let outcome = opt_cell(row, "outcome").and_then(|raw| {
        let wanted = values::loose(&raw);
        let found = rtype.outcome_list().into_iter().find(|o| {
            values::loose(&o.key) == wanted
                || values::loose(&outcome_label(rtype, &o.key, o.label.as_deref())) == wanted
        });
        if found.is_none() {
            errors.push(error(
                Some("outcome"),
                format!(
                    "'{raw}' isn't an outcome of {}. Use one of: {}",
                    entry.label,
                    rtype
                        .outcome_list()
                        .iter()
                        .map(|o| outcome_label(rtype, &o.key, o.label.as_deref()))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ));
        }
        found.map(|o| o.key)
    });

    // Cost, in the row's currency, else the record's, else the workspace's.
    let currency_cell = opt_cell(row, "currency").map(|c| c.to_ascii_uppercase());
    if let Some(code) = &currency_cell {
        if money::currency(code).is_none() {
            errors.push(error(
                Some("currency"),
                format!("'{code}' is not an ISO 4217 currency code"),
            ));
        }
    }
    let cost = match opt_cell(row, "cost") {
        Some(raw) => {
            let code = currency_cell
                .clone()
                .or_else(|| existing.and_then(|r| r.cost_currency.clone()))
                .or_else(|| lookups.currency.clone());
            match code {
                None => {
                    errors.push(error(
                        Some("cost"),
                        "no currency for this cost: add a currency column or set the workspace currency under Settings, Assets",
                    ));
                    None
                }
                Some(code) if money::currency(&code).is_none() => None,
                Some(code) => match money::parse_amount(&raw, &code, mark) {
                    Ok(minor) if minor < 0 => {
                        errors.push(error(Some("cost"), format!("'{raw}' is negative")));
                        None
                    }
                    Ok(minor) => Some(Cost {
                        minor,
                        currency: code,
                    }),
                    Err(e) => {
                        errors.push(error(Some("cost"), format!("'{raw}': {e} ({code})")));
                        None
                    }
                },
            }
        }
        None => {
            if let Some(code) = &currency_cell {
                // A currency alone says nothing to record.
                dropped.push(DroppedCell {
                    column: "currency".to_string(),
                    source_column: None,
                    kind: entry.slug().to_string(),
                    value: Some(code.clone()),
                    kept: false,
                });
            }
            None
        }
    };

    // A ticket number that names no ticket is left off.
    let ticket = opt_cell(row, "ticket").and_then(|raw| {
        let found = ticket_number(&raw).and_then(|n| lookups.tickets.get(&n).copied());
        if found.is_none() {
            dropped.push(DroppedCell {
                column: "ticket".to_string(),
                source_column: None,
                kind: entry.slug().to_string(),
                value: Some(raw),
                kept: false,
            });
        }
        found
    });

    // People, as Match values answered them, else by email, under the rules
    // the record API names them by: whoever held the asset is a member, and
    // whoever did the work can work tickets here (unless the record already
    // names them). A cell naming anyone else is left off. On a new record it
    // records nobody: the sheet said who it was, so nobody else is put in
    // their place. On a record being updated it keeps the person stored, as
    // a blank cell does, so running a sheet again after someone has left
    // keeps their name on their past work. `None` leaves the field as it is.
    let may_name = |field: &str, user: Uuid| match field {
        "performed_by" => {
            lookups.performers.contains(&user)
                || existing.is_some_and(|r| r.performed_by_uuid == Some(user))
        }
        _ => true,
    };
    let person = |field: &str, dropped: &mut Vec<DroppedCell>| -> Option<Option<Uuid>> {
        let raw = opt_cell(row, field)?;
        let linked = match options
            .lookups
            .get(field)
            .and_then(|c| c.get(""))
            .and_then(|v| v.get(&normalize_value(&raw)))
        {
            Some(ValueAnswer::Match { target }) => target
                .as_str()
                .and_then(|t| Uuid::parse_str(t).ok())
                .filter(|u| lookups.member_emails.contains_key(u)),
            Some(ValueAnswer::LeaveBlank) => None,
            _ => lookups.member(&raw),
        }
        .filter(|u| may_name(field, *u));
        if linked.is_none() {
            dropped.push(DroppedCell {
                column: field.to_string(),
                source_column: None,
                kind: entry.slug().to_string(),
                value: Some(raw),
                kept: existing.is_some(),
            });
            if existing.is_some() {
                return None;
            }
        }
        Some(linked)
    };
    let performed_by = person("performed_by", &mut dropped);
    let holder = person("holder", &mut dropped);
    let location = opt_cell(row, "location");

    // The replacement, by its tag.
    let replacement = opt_cell(row, "replacement_asset_tag").and_then(|tag| {
        match holders(&lookups.tags, &lookups.tags_lower, &tag) {
            [id] => Some(*id),
            [] => {
                errors.push(error(
                    Some("replacement_asset_tag"),
                    format!("{NO_ASSET_WITH_TAG}'{tag}'"),
                ));
                None
            }
            ids => {
                errors.push(error(
                    Some("replacement_asset_tag"),
                    shared(lookups, "tag", &tag, ids),
                ));
                None
            }
        }
    });

    // The type's own fields.
    let mut attributes = existing.map_or_else(Map::new, |r| {
        r.attributes.as_object().cloned().unwrap_or_default()
    });
    let mut attributes_written = false;
    for def in &lookups.fields {
        let raw = cell(row, &def.id);
        if raw.is_empty() {
            continue;
        }
        let Some(prop) = def.prop(entry.slug()) else {
            if !options.constants.iter().any(|c| c.field == def.id) {
                dropped.push(DroppedCell {
                    column: def.id.clone(),
                    source_column: None,
                    kind: entry.slug().to_string(),
                    value: None,
                    kept: false,
                });
            }
            continue;
        };
        match parse_field(&raw, prop, lookups, options) {
            Ok(value) => {
                attributes.insert(def.key.clone(), value);
                attributes_written = true;
            }
            Err(message) => errors.push(error(Some(&def.id), message)),
        }
    }

    // A type change: the record's fields and outcome the new type doesn't
    // have go only when the row gives the new type's own; else the row says
    // what would be lost.
    if let Some(record) = existing.filter(|r| r.record_type_id != rtype.id) {
        let keys: HashSet<String> = rtype
            .field_schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default();
        let displaced: Vec<String> = record
            .attributes
            .as_object()
            .map(|a| a.keys().filter(|k| !keys.contains(*k)).cloned().collect())
            .unwrap_or_default();
        if !displaced.is_empty() {
            if attributes_written {
                attributes.retain(|k, _| keys.contains(k));
            } else {
                let names: Vec<String> = displaced
                    .iter()
                    .map(|k| {
                        lookups
                            .fields
                            .iter()
                            .find(|d| &d.key == k)
                            .map_or_else(|| humanize(k), |d| d.title.clone())
                    })
                    .collect();
                errors.push(error(
                    Some("record_type"),
                    format!(
                        "{} has no {}, which this record holds; give the row {}'s own fields to change its type",
                        entry.label,
                        names.join(", "),
                        entry.label
                    ),
                ));
            }
        }
        if let Some(stored) = record.outcome.as_deref() {
            if outcome.is_none() && rtype.outcome(stored).is_none() {
                let old = lookups.type_by_id(record.record_type_id);
                let shown = old.map_or_else(
                    || stored.to_string(),
                    |t| {
                        let own = t.rtype.outcome(stored).and_then(|o| o.label);
                        outcome_label(&t.rtype, stored, own.as_deref())
                    },
                );
                errors.push(error(
                    Some("outcome"),
                    format!(
                        "'{shown}' isn't an outcome of {}; give the row one of its outcomes to change its type",
                        entry.label
                    ),
                ));
            }
        }
    }

    if !errors.is_empty() {
        return Ok(Err(errors));
    }

    let text = |field: &str| opt_cell(row, field);
    let entered_snapshot = holder.is_some() || location.is_some();
    let planned = match existing {
        None => {
            let Some(occurred) = occurred else {
                return Ok(Err(vec![error(
                    Some("occurred_on"),
                    "date of work is required",
                )]));
            };
            let completed = completed.or(match options.blank_completion {
                BlankCompletion::SameDay => Some(WorkDate::Day(day_of(occurred, tz))),
                BlankCompletion::Open => None,
            });
            let input = NewRecord {
                asset_id: asset.id,
                record_type_id: rtype.id,
                summary: text("summary"),
                notes: text("notes"),
                occurred,
                completed,
                outcome,
                vendor: text("vendor"),
                performed_by_uuid: performed_by.flatten(),
                reference: text("reference"),
                cost: cost.clone(),
                ticket_id: ticket,
                replacement_asset_id: replacement,
                snapshot: entered_snapshot.then(|| Snapshot {
                    holder_uuid: holder.flatten(),
                    location: location.clone(),
                }),
                attributes: Value::Object(attributes),
                source: RecordSource::Import,
                source_ref: source_ref.clone(),
                import_job_id: None,
                rule_id: None,
                lifecycle_event_id: None,
                recorded_by: None,
            };
            match record_repo::check_new(conn, &input, asset, rtype, tz) {
                Ok((draft, warnings)) => Planned {
                    action: Action::Create(input.clone()),
                    asset_id: asset.id,
                    type_slug: entry.slug().to_string(),
                    occurred_on: draft.occurred_on,
                    likeness: Likeness::of(
                        draft.reference.as_deref(),
                        draft.cost_minor.zip(draft.cost_currency.clone()),
                        draft.summary.as_deref(),
                    ),
                    cost: draft.cost_currency.clone().zip(draft.cost_minor),
                    fields_written: BTreeMap::new(),
                    dropped,
                    warnings,
                },
                Err(RecordError::Db(e)) => return Err(e),
                Err(e) => return Ok(Err(vec![record_error(e, entry, lookups)])),
            }
        }
        Some(record) => {
            let changes = RecordChanges {
                record_type_id: Some(rtype.id),
                summary: text("summary").map(Some),
                notes: text("notes").map(Some),
                occurred,
                completed: completed.map(Some),
                outcome: outcome.map(Some),
                vendor: text("vendor").map(Some),
                performed_by_uuid: performed_by,
                reference: text("reference").map(Some),
                cost: cost.clone().map(Some),
                ticket_id: ticket.map(Some),
                replacement_asset_id: replacement.map(Some),
                // A blank half keeps what the record holds.
                snapshot: entered_snapshot.then(|| Snapshot {
                    holder_uuid: match holder {
                        Some(named) => named,
                        None => record.holder_uuid,
                    },
                    location: location.clone().or_else(|| record.location.clone()),
                }),
                attributes: attributes_written.then_some(Value::Object(attributes)),
            };
            match record_repo::check_update(conn, record, changes.clone(), asset, rtype, tz, None) {
                Ok((draft, warnings)) => {
                    let before = record_values(&RecordView::of_record(record), lookups);
                    let after = record_values(&RecordView::of_draft(&draft, record), lookups);
                    let fields_written = written_fields(&before, &after);
                    let action = if fields_written.is_empty() {
                        Action::Unchanged(record.id)
                    } else {
                        Action::Edit {
                            id: record.id,
                            version: record.version,
                            changes,
                        }
                    };
                    Planned {
                        action,
                        asset_id: asset.id,
                        type_slug: entry.slug().to_string(),
                        occurred_on: draft.occurred_on,
                        likeness: Likeness::default(),
                        cost: draft.cost_currency.clone().zip(draft.cost_minor),
                        fields_written,
                        dropped,
                        warnings,
                    }
                }
                Err(RecordError::Db(e)) => return Err(e),
                Err(e) => return Ok(Err(vec![record_error(e, entry, lookups)])),
            }
        }
    };
    Ok(Ok(planned))
}

/// Each field a write changes, before and after: the fields of both sides,
/// so a field a type change displaces is there with a blank after. A cost
/// and its currency go together, so the previous values put back an amount
/// in the currency it was in.
fn written_fields(
    before: &BTreeMap<String, Option<String>>,
    after: &BTreeMap<String, Option<String>>,
) -> BTreeMap<String, FieldChange> {
    let change = |f: &str| FieldChange {
        before: before.get(f).cloned().flatten(),
        after: after.get(f).cloned().flatten(),
    };
    let mut out: BTreeMap<String, FieldChange> = before
        .keys()
        .chain(after.keys())
        .map(|f| (f.clone(), change(f)))
        .filter(|(_, c)| c.before != c.after)
        .collect();
    if out.contains_key("cost") || out.contains_key("currency") {
        for f in COST_FIELDS {
            out.insert(f.to_string(), change(f));
        }
    }
    out
}

/// The fields a cost is written in, which a write and the previous values
/// keep together.
pub const COST_FIELDS: [&str; 2] = ["cost", "currency"];

/// One cell of a type field, as the field's schema types it. The schema's
/// other rules are checked with the whole record.
fn parse_field(
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
                .map(|item| parse_field(item, &items, lookups, options))
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
        _ => {
            if prop.get("enum").is_some() {
                let choices = enum_options(prop);
                let wanted = values::loose(raw);
                return choices
                    .iter()
                    .find(|c| values::loose(c) == wanted)
                    .cloned()
                    .map(Value::String)
                    .ok_or_else(|| format!("'{raw}' is not one of: {}", choices.join(", ")));
            }
            let order = options.date_order;
            let text = match prop.get("format").and_then(Value::as_str) {
                Some("date") => values::parse_date(raw, order)
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .ok_or_else(|| {
                        format!(
                            "'{raw}' is not a valid date; use {}",
                            values::date_hint(order)
                        )
                    })?,
                Some("date-time") => match read_work_date(raw, order, lookups.tz) {
                    Some(WorkDate::At(at)) => {
                        at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true)
                    }
                    Some(WorkDate::Day(day)) => day
                        .and_time(NaiveTime::MIN)
                        .and_utc()
                        .to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
                    None => {
                        return Err(format!(
                            "'{raw}' is not a valid date and time; use YYYY-MM-DD HH:MM or {}",
                            values::date_hint(order)
                        ))
                    }
                },
                Some("user-uuid") => lookups
                    .member(raw)
                    .map(|u| u.to_string())
                    .ok_or_else(|| format!("'{raw}' is not a member of this workspace"))?,
                Some("asset-ref") => match holders(&lookups.tags, &lookups.tags_lower, raw) {
                    [id] => id.to_string(),
                    [] => match raw.parse::<i32>() {
                        Ok(id) if id > 0 => id.to_string(),
                        _ => return Err(format!("no asset has the tag '{raw}'")),
                    },
                    ids => return Err(shared(lookups, "tag", raw, ids)),
                },
                _ => raw.to_string(),
            };
            Ok(Value::String(text))
        }
    }
}

/// A refusal from the record rules as a row error, on the column it is
/// about.
fn record_error(e: RecordError, entry: &TypeEntry, lookups: &Lookups) -> CellError {
    let label = &entry.label;
    match e {
        RecordError::RecordTypeArchived => error(
            Some("record_type"),
            format!("{label} is archived; restore it under Settings to import records of it"),
        ),
        RecordError::RecordTypeNotForKind { kind } => error(
            Some("record_type"),
            format!(
                "{label} isn't offered for {} assets; change the type's kinds under Settings",
                lookups.kind_label(&kind)
            ),
        ),
        RecordError::Attributes(e) => attribute_error(&e, label, lookups),
        RecordError::Required { field } => {
            error(Some(field), format!("required for {label} records"))
        }
        RecordError::UnknownOutcome { outcome } => error(
            Some("outcome"),
            format!("'{outcome}' isn't an outcome of {label}"),
        ),
        RecordError::OutcomeWhileOpen => error(
            Some("outcome"),
            "open work has no outcome; give the row a completion date",
        ),
        RecordError::ReplacementNeedsReplacedOutcome => error(
            Some("replacement_asset_tag"),
            "a replacement asset needs an outcome that means the asset was replaced",
        ),
        RecordError::ReplacementIsSelf => error(
            Some("replacement_asset_tag"),
            "an asset can't replace itself",
        ),
        RecordError::ReplacementNotFound => {
            error(Some("replacement_asset_tag"), "replacement asset not found")
        }
        RecordError::TicketNotFound => error(Some("ticket"), "ticket not found"),
        RecordError::TooEarly => error(Some("occurred_on"), "dates start at 1900-01-01"),
        RecordError::InFuture => error(Some("occurred_on"), "work can't be dated after tomorrow"),
        RecordError::CompletedBeforeOccurred => error(
            Some("completed_on"),
            "completion is before the work started",
        ),
        RecordError::UnknownCurrency { currency } => error(
            Some("currency"),
            format!("'{currency}' is not an ISO 4217 currency code"),
        ),
        RecordError::NegativeCost => error(Some("cost"), "cost can't be negative"),
        RecordError::TooLong { field, max } => {
            error(Some(field), format!("longer than {max} characters"))
        }
        RecordError::SourceRefExists { id } => error(
            Some("source_ref"),
            format!("record {id} already has this source reference"),
        ),
        other => error(None, other.to_string()),
    }
}

/// A type field check that failed, on the field it is about.
fn attribute_error(e: &AttributeError, label: &str, lookups: &Lookups) -> CellError {
    use AttributeError as E;
    let on = |key: &String, message: String| error(Some(&lookups.field_id(key)), message);
    match e {
        E::MissingRequired(key) => on(key, format!("required for {label} records")),
        E::UnknownProperty(key) => on(key, format!("{label} records have no field '{key}'")),
        E::WrongType {
            property, expected, ..
        } => on(property, format!("not a {expected}")),
        E::NotInEnum { property, allowed } => {
            let allowed: Vec<String> = allowed.iter().filter_map(value_text).collect();
            on(property, format!("not one of: {}", allowed.join(", ")))
        }
        E::TooShort { property, min } => on(property, format!("shorter than {min} characters")),
        E::TooLong { property, max } => on(property, format!("longer than {max} characters")),
        E::PatternMismatch { property } => {
            on(property, format!("doesn't match the pattern {label} sets"))
        }
        E::BadFormat { property, format } => on(property, format!("not a valid {format}")),
        E::BelowMinimum { property, min } => on(property, format!("below the minimum of {min}")),
        E::AboveMaximum { property, max } => on(property, format!("above the maximum of {max}")),
        E::NotMultipleOf { property, step } => on(property, format!("not a multiple of {step}")),
        E::NotObject => error(None, e.to_string()),
    }
}

/// A write the database refused at commit, on its row.
fn write_error(row: usize, e: RecordError) -> CommitError {
    match e {
        RecordError::Db(e) => CommitError::Db(e),
        RecordError::SourceRefExists { id } => CommitError::Row {
            row,
            column: Some("source_ref".to_string()),
            message: format!("record {id} already has this source reference"),
        },
        RecordError::VersionConflict { .. } => CommitError::Row {
            row,
            column: None,
            message: "the record changed while importing; check the rows again".to_string(),
        },
        other => CommitError::Row {
            row,
            column: None,
            message: other.to_string(),
        },
    }
}

/// The values of a record an import can write, whether stored or drafted.
struct RecordView<'a> {
    record_type_id: i32,
    summary: &'a Option<String>,
    notes: &'a Option<String>,
    occurred_on: NaiveDate,
    occurred_at: Option<DateTime<Utc>>,
    completed_on: Option<NaiveDate>,
    completed_at: Option<DateTime<Utc>>,
    outcome: &'a Option<String>,
    vendor: &'a Option<String>,
    performed_by_uuid: Option<Uuid>,
    reference: &'a Option<String>,
    cost_minor: Option<i64>,
    cost_currency: &'a Option<String>,
    ticket_id: Option<i32>,
    replacement_asset_id: Option<i32>,
    holder_uuid: Option<Uuid>,
    location: &'a Option<String>,
    attributes: &'a Value,
    source_ref: &'a Option<String>,
}

impl<'a> RecordView<'a> {
    fn of_record(r: &'a AssetRecord) -> Self {
        Self {
            record_type_id: r.record_type_id,
            summary: &r.summary,
            notes: &r.notes,
            occurred_on: r.occurred_on,
            occurred_at: r.occurred_at,
            completed_on: r.completed_on,
            completed_at: r.completed_at,
            outcome: &r.outcome,
            vendor: &r.vendor,
            performed_by_uuid: r.performed_by_uuid,
            reference: &r.reference,
            cost_minor: r.cost_minor,
            cost_currency: &r.cost_currency,
            ticket_id: r.ticket_id,
            replacement_asset_id: r.replacement_asset_id,
            holder_uuid: r.holder_uuid,
            location: &r.location,
            attributes: &r.attributes,
            source_ref: &r.source_ref,
        }
    }

    fn of_draft(d: &'a AssetRecordEdit, r: &'a AssetRecord) -> Self {
        Self {
            record_type_id: d.record_type_id,
            summary: &d.summary,
            notes: &d.notes,
            occurred_on: d.occurred_on,
            occurred_at: d.occurred_at,
            completed_on: d.completed_on,
            completed_at: d.completed_at,
            outcome: &d.outcome,
            vendor: &d.vendor,
            performed_by_uuid: d.performed_by_uuid,
            reference: &d.reference,
            cost_minor: d.cost_minor,
            cost_currency: &d.cost_currency,
            ticket_id: d.ticket_id,
            replacement_asset_id: d.replacement_asset_id,
            holder_uuid: d.holder_uuid,
            location: &d.location,
            attributes: &d.attributes,
            source_ref: &r.source_ref,
        }
    }
}

/// A record's importable fields as text by field id, blank as `None`, as a
/// cell of the file would hold them: people by email, the replacement by
/// its tag, the cost as a plain decimal.
fn record_values(r: &RecordView, lookups: &Lookups) -> BTreeMap<String, Option<String>> {
    let tz = lookups.tz;
    let text = |v: &Option<String>| v.clone().filter(|v| !v.is_empty());
    let mut out = BTreeMap::new();
    out.insert(
        "record_type".to_string(),
        lookups
            .type_by_id(r.record_type_id)
            .map(|t| t.slug().to_string()),
    );
    out.insert("summary".to_string(), text(r.summary));
    out.insert("notes".to_string(), text(r.notes));
    out.insert(
        "occurred_on".to_string(),
        Some(work_date_text(r.occurred_on, r.occurred_at, tz)),
    );
    out.insert(
        "completed_on".to_string(),
        r.completed_on
            .map(|on| work_date_text(on, r.completed_at, tz)),
    );
    out.insert("outcome".to_string(), text(r.outcome));
    out.insert("vendor".to_string(), text(r.vendor));
    out.insert(
        "performed_by".to_string(),
        r.performed_by_uuid.map(|u| lookups.member_text(u)),
    );
    out.insert("reference".to_string(), text(r.reference));
    out.insert(
        "cost".to_string(),
        r.cost_minor
            .zip(r.cost_currency.as_deref())
            .and_then(|(m, c)| money::format_minor(m, c).ok()),
    );
    out.insert("currency".to_string(), text(r.cost_currency));
    out.insert(
        "ticket".to_string(),
        r.ticket_id.map(|id| {
            lookups
                .ticket_numbers
                .get(&id)
                .copied()
                .unwrap_or(id)
                .to_string()
        }),
    );
    out.insert(
        "replacement_asset_tag".to_string(),
        r.replacement_asset_id
            .and_then(|id| lookups.asset_tag_of(id)),
    );
    out.insert(
        "holder".to_string(),
        r.holder_uuid.map(|u| lookups.member_text(u)),
    );
    out.insert("location".to_string(), text(r.location));
    let props = lookups
        .type_by_id(r.record_type_id)
        .and_then(|t| t.rtype.field_schema.get("properties"))
        .and_then(Value::as_object);
    if let Some(attributes) = r.attributes.as_object() {
        for (key, value) in attributes {
            let prop = props.and_then(|p| p.get(key));
            out.insert(lookups.field_id(key), field_text(value, prop, lookups));
        }
    }
    out.insert("source_ref".to_string(), text(r.source_ref));
    out
}

/// A type field's value as a cell writes it: an asset reference by the
/// asset's tag where it has one, else its id, as [`parse_field`] reads it
/// back.
fn field_text(value: &Value, prop: Option<&Value>, lookups: &Lookups) -> Option<String> {
    let is_ref = |p: Option<&Value>| {
        p.and_then(|p| p.get("format")).and_then(Value::as_str) == Some("asset-ref")
    };
    let tag = |v: &Value| -> Option<String> {
        let id = match v {
            Value::String(s) => s.trim().parse::<i32>().ok(),
            Value::Number(n) => n.as_i64().and_then(|n| i32::try_from(n).ok()),
            _ => None,
        }?;
        lookups.asset_tag_of(id).filter(|t| !t.is_empty())
    };
    if is_ref(prop) {
        return tag(value).or_else(|| value_text(value));
    }
    if let (Value::Array(items), true) = (value, is_ref(prop.and_then(|p| p.get("items")))) {
        let texts: Vec<String> = items
            .iter()
            .filter_map(|v| tag(v).or_else(|| value_text(v)))
            .collect();
        return (!texts.is_empty()).then(|| texts.join("; "));
    }
    value_text(value)
}

/// Records' importable fields as text by field id, by record id, as the
/// commit records them: what Restore previous values holds the import's
/// writes up against.
pub fn current_field_values(
    conn: &mut DbConnection,
    ids: &[i32],
) -> Result<CurrentValues, diesel::result::Error> {
    use crate::schema::asset_records;
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let records: Vec<AssetRecord> = asset_records::table
        .filter(asset_records::id.eq_any(ids))
        .load(conn)?;
    let empty = ParsedCsv {
        headers: Vec::new(),
        rows: Vec::new(),
        delimiter: b',',
    };
    let mut lookups = Lookups::load(conn, &empty)?;
    let linked: Vec<i32> = records.iter().filter_map(|r| r.ticket_id).collect();
    lookups
        .ticket_numbers
        .extend(load_ticket_numbers(conn, &linked)?);
    Ok(records
        .iter()
        .map(|r| (r.id, record_values(&RecordView::of_record(r), &lookups)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tz() -> Tz {
        "Australia/Sydney".parse().expect("zone")
    }

    #[test]
    fn a_date_reads_as_a_day_and_a_time_in_the_workspace_zone() {
        let day = NaiveDate::from_ymd_opt(2019, 3, 14).expect("date");
        assert_eq!(
            read_work_date("2019-03-14", DateOrder::Iso, tz()),
            Some(WorkDate::Day(day))
        );
        assert_eq!(
            read_work_date("14/03/2019", DateOrder::Dmy, tz()),
            Some(WorkDate::Day(day))
        );
        // A workbook's midnight is a date.
        assert_eq!(
            read_work_date("2019-03-14T00:00:00", DateOrder::Iso, tz()),
            Some(WorkDate::Day(day))
        );
        // 09:30 in Sydney (UTC+11 in March) is 22:30 the day before in UTC.
        let Some(WorkDate::At(at)) = read_work_date("14/03/2019 09:30", DateOrder::Dmy, tz())
        else {
            panic!("a time reads as a moment");
        };
        assert_eq!(at.to_rfc3339(), "2019-03-13T22:30:00+00:00");
        assert_eq!(day_of(WorkDate::At(at), tz()), day);
        assert_eq!(
            work_date_text(day, Some(at), tz()),
            "2019-03-14T09:30:00+11:00"
        );
        assert_eq!(
            read_work_date("2019-03-14T09:30:00+11:00", DateOrder::Iso, tz()),
            Some(WorkDate::At(at))
        );
        // Seconds and fractions are kept.
        let Some(WorkDate::At(precise)) =
            read_work_date("2019-03-14 09:30:15.25", DateOrder::Iso, tz())
        else {
            panic!("a precise time reads");
        };
        assert_eq!(
            work_date_text(day, Some(precise), tz()),
            "2019-03-14T09:30:15.250+11:00"
        );
        // 02:30 on 1 October 2023 doesn't exist in Sydney (clocks go from
        // 02:00 to 03:00): it reads as the first moment after the gap.
        let Some(WorkDate::At(gap)) = read_work_date("2023-10-01 02:30", DateOrder::Iso, tz())
        else {
            panic!("a time in the gap reads");
        };
        assert_eq!(gap.to_rfc3339(), "2023-09-30T16:30:00+00:00");
        // 02:30 on 2 April 2023 happens twice: the first of the two.
        let Some(WorkDate::At(repeated)) = read_work_date("2023-04-02 02:30", DateOrder::Iso, tz())
        else {
            panic!("a repeated time reads");
        };
        assert_eq!(repeated.to_rfc3339(), "2023-04-01T15:30:00+00:00");
        assert!(read_work_date("next tuesday", DateOrder::Iso, tz()).is_none());
    }

    #[test]
    fn ticket_numbers_read_with_or_without_a_hash() {
        assert_eq!(ticket_number("#412"), Some(412));
        assert_eq!(ticket_number(" 412 "), Some(412));
        assert_eq!(ticket_number("T-412"), None);
        assert_eq!(ticket_number("0"), None);
    }

    #[test]
    fn likeness_falls_through_when_either_side_lacks_a_value() {
        let cost = |minor| Some((minor, "AUD".to_string()));
        let rma = Likeness::of(Some(" RMA-1 "), cost(125_050), Some("Lens"));
        // Both have a reference: it decides.
        assert!(rma.same_work(&Likeness::of(Some("rma-1"), cost(1), None)));
        assert!(!rma.same_work(&Likeness::of(Some("RMA-2"), cost(125_050), Some("Lens"))));
        // One has none: the cost decides, then the summary.
        assert!(rma.same_work(&Likeness::of(None, cost(125_050), None)));
        assert!(!rma.same_work(&Likeness::of(None, cost(1), Some("Lens"))));
        assert!(rma.same_work(&Likeness::of(None, None, Some(" lens "))));
        assert!(!rma.same_work(&Likeness::of(None, None, None)));
        assert!(Likeness::of(Some(""), None, None).same_work(&Likeness::of(None, None, None)));
    }

    #[test]
    fn catalogue_types_are_named_in_english() {
        let entry = record_catalogue::entry("preventative_maintenance").expect("entry");
        let rtype = catalogue_type(entry, -1);
        assert_eq!(type_label(&rtype), "Preventative maintenance");
        let repair = catalogue_type(record_catalogue::entry("repair").expect("repair"), -2);
        assert_eq!(
            outcome_label(&repair, "no_fault_found", None),
            "No fault found"
        );
        assert_eq!(outcome_label(&repair, "repaired", Some("Fixed")), "Fixed");
        let types = vec![
            TypeEntry {
                label: type_label(&rtype),
                rtype,
                from_catalogue: true,
            },
            TypeEntry {
                label: type_label(&repair),
                rtype: repair,
                from_catalogue: true,
            },
        ];
        assert_eq!(
            find_type(&types, "Preventative Maintenance").map(|t| t.slug()),
            Some("preventative_maintenance")
        );
        assert_eq!(
            find_type(&types, "REPAIR").map(|t| t.slug()),
            Some("repair")
        );
        assert!(find_type(&types, "Calibration").is_none());
    }

    #[test]
    fn type_fields_colliding_with_shared_columns_take_a_prefix() {
        let mut rtype = catalogue_type(record_catalogue::entry("repair").expect("repair"), -1);
        rtype.field_schema = serde_json::json!({
            "type": "object",
            "properties": { "summary": { "type": "string" }, "fault": { "type": "string" } }
        });
        rtype.field_order = vec![Some("fault".into()), Some("summary".into())];
        let types = vec![TypeEntry {
            label: "Repair".into(),
            rtype,
            from_catalogue: false,
        }];
        let ids: Vec<String> = field_defs(&types).into_iter().map(|d| d.id).collect();
        assert_eq!(ids, vec!["fault".to_string(), "attr:summary".to_string()]);
    }
}
