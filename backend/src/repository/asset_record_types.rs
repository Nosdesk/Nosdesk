//! Asset record types: a workspace's kinds of work on its assets.
//!
//! A type has its own fields (a schema in the custom-fields subset, with an
//! explicit order because jsonb sorts keys), says which shared fields it
//! requires or hides, names its outcomes on four fixed categories, and may
//! name the asset status whose transition opens a record of it.
//!
//! No type is seeded. The built-ins (repair, inspection, preventative
//! maintenance, firmware update) live in
//! [`crate::services::assets::record_catalogue`] and reach a workspace
//! through [`add_from_catalogue`]. Built-ins can be renamed, edited and
//! archived, never deleted; a used type is archived rather than deleted
//! (records hold it with `ON DELETE RESTRICT`).
//!
//! Types are workspace configuration, not a sync aggregate: pickers read
//! them over REST and refetch after a save, and the audit trigger records
//! every change.

use std::collections::HashSet;

use chrono::Utc;
use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use regex::Regex;
use serde_json::{json, Value};
use std::sync::LazyLock;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    AssetRecordType, AssetRecordTypeChange, AssetStatus, CoreFieldMode, NewAssetRecordType,
    RecordOutcome, CORE_FIELDS,
};
use crate::schema::{asset_kinds, asset_record_types, asset_records};
use crate::services::assets::record_catalogue;
use crate::services::custom_fields::schema::{
    validate_attributes, validate_schema, AttributeSchemaError,
};

static KEY: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"^[a-z][a-z0-9_]{0,63}$").expect("key pattern")
});

static COLOR: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    Regex::new(r"^#[0-9A-Fa-f]{6}$").expect("colour pattern")
});

/// The longest name, and the longest icon name.
const LABEL_MAX: usize = 255;
const ICON_MAX: usize = 64;

/// A name, icon and colour that fit their columns.
fn check_display(
    label: Option<&str>,
    icon: Option<&str>,
    color: Option<&str>,
) -> Result<(), RecordTypeError> {
    for (field, value, max) in [("label", label, LABEL_MAX), ("icon", icon, ICON_MAX)] {
        if value.is_some_and(|v| v.chars().count() > max) {
            return Err(RecordTypeError::TooLong { field, max });
        }
    }
    match color {
        Some(c) if !COLOR.is_match(c) => Err(RecordTypeError::InvalidColor(c.to_string())),
        _ => Ok(()),
    }
}

/// Why a record type write was refused.
#[derive(Debug)]
pub enum RecordTypeError {
    NotFound,
    /// A slug or outcome key isn't lower case letters, digits and
    /// underscores, starting with a letter.
    InvalidKey(String),
    SlugTaken,
    /// The slug belongs to a catalogue entry; add it from the catalogue.
    SlugReserved,
    /// A type that isn't built in needs a name.
    LabelRequired,
    Schema(AttributeSchemaError),
    /// `core_fields` names a field that isn't shared, or a mode that isn't
    /// one.
    CoreFields(String),
    /// A repeated outcome key, a category that isn't one, or an outcome with
    /// no name and no catalogue string.
    Outcomes(String),
    UnknownKind(String),
    UnknownStatus(String),
    /// Another live type already opens with this status.
    StatusTaken,
    /// The change would leave records that no longer fit: `count` of them,
    /// with a sample.
    InvalidRecords {
        count: usize,
        samples: Vec<Value>,
    },
    /// Built-in types are archived, never deleted.
    Builtin,
    /// Records use the type; archive it instead.
    InUse {
        count: i64,
    },
    NotInCatalogue,
    /// A name or icon longer than its column holds, in characters.
    TooLong {
        field: &'static str,
        max: usize,
    },
    /// A colour that isn't `#rrggbb`.
    InvalidColor(String),
    Db(Error),
}

impl From<Error> for RecordTypeError {
    fn from(e: Error) -> Self {
        RecordTypeError::Db(e)
    }
}

/// A type to create.
#[derive(Debug, Clone)]
pub struct NewRecordTypeInput {
    pub slug: String,
    pub label: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub field_schema: Value,
    /// Display order of the fields; keys the schema lacks are dropped and
    /// keys it leaves out are appended.
    pub field_order: Vec<String>,
    pub core_fields: Value,
    pub outcomes: Vec<RecordOutcome>,
    pub applies_to_kinds: Option<Vec<String>>,
    pub opens_with_status: Option<String>,
    pub sort_order: i32,
    pub created_by: Option<Uuid>,
}

/// An edit to a type. Outer `None` leaves a field as it is; for a field that
/// may be empty, `Some(None)` clears it.
#[derive(Debug, Clone, Default)]
pub struct RecordTypeChanges {
    /// `Some(None)` puts a built-in back to its catalogue name.
    pub label: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub icon: Option<Option<String>>,
    pub color: Option<Option<String>>,
    pub field_schema: Option<Value>,
    pub field_order: Option<Vec<String>>,
    pub core_fields: Option<Value>,
    pub outcomes: Option<Vec<RecordOutcome>>,
    pub applies_to_kinds: Option<Option<Vec<String>>>,
    pub opens_with_status: Option<Option<String>>,
    pub sort_order: Option<i32>,
}

/// The workspace's types by `sort_order`, archived ones too when asked.
pub fn list(conn: &mut DbConnection, include_archived: bool) -> QueryResult<Vec<AssetRecordType>> {
    let mut query = asset_record_types::table.into_boxed();
    if !include_archived {
        query = query.filter(asset_record_types::archived_at.is_null());
    }
    query
        .order((
            asset_record_types::sort_order.asc(),
            asset_record_types::id.asc(),
        ))
        .load(conn)
}

pub fn get(conn: &mut DbConnection, id: i32) -> QueryResult<Option<AssetRecordType>> {
    asset_record_types::table.find(id).first(conn).optional()
}

pub fn get_by_slug(conn: &mut DbConnection, slug: &str) -> QueryResult<Option<AssetRecordType>> {
    asset_record_types::table
        .filter(asset_record_types::slug.eq(slug))
        .first(conn)
        .optional()
}

/// The live type a transition into `status` opens a record of, if any.
pub fn opened_by_status(
    conn: &mut DbConnection,
    status: &str,
) -> QueryResult<Option<AssetRecordType>> {
    asset_record_types::table
        .filter(asset_record_types::opens_with_status.eq(status))
        .filter(asset_record_types::archived_at.is_null())
        .first(conn)
        .optional()
}

/// How many records use type `id`.
pub fn count_records(conn: &mut DbConnection, id: i32) -> QueryResult<i64> {
    asset_records::table
        .filter(asset_records::record_type_id.eq(id))
        .count()
        .get_result(conn)
}

/// Records of type `id` that wouldn't fit `field_schema` and `outcomes`:
/// how many, and up to `sample_limit` of them with the reason. Run before a
/// schema or outcome change saves.
pub fn invalid_records(
    conn: &mut DbConnection,
    id: i32,
    field_schema: &Value,
    outcomes: &[RecordOutcome],
    sample_limit: usize,
) -> QueryResult<(usize, Vec<Value>)> {
    let rows: Vec<(i32, i32, Value, Option<String>)> = asset_records::table
        .filter(asset_records::record_type_id.eq(id))
        .select((
            asset_records::id,
            asset_records::asset_id,
            asset_records::attributes,
            asset_records::outcome,
        ))
        .order(asset_records::id.asc())
        .load(conn)?;
    let keys: HashSet<&str> = outcomes.iter().map(|o| o.key.as_str()).collect();
    let mut count = 0usize;
    let mut samples = Vec::new();
    for (record_id, asset_id, attributes, outcome) in rows {
        let error = match validate_attributes(field_schema, &attributes) {
            Err(e) => Some(e.to_string()),
            Ok(()) => outcome
                .filter(|o| !keys.contains(o.as_str()))
                .map(|o| format!("outcome `{o}` is no longer one of the type's")),
        };
        if let Some(error) = error {
            count += 1;
            if samples.len() < sample_limit {
                samples.push(json!({ "id": record_id, "asset_id": asset_id, "error": error }));
            }
        }
    }
    Ok((count, samples))
}

/// `order` as a permutation of the schema's property names: unknown and
/// repeated names dropped, missing ones appended.
fn normalise_order(schema: &Value, order: &[String]) -> Vec<String> {
    let keys: Vec<String> = schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let mut out: Vec<String> = order
        .iter()
        .filter(|k| keys.contains(k) && seen.insert(k.as_str().to_string()))
        .cloned()
        .collect();
    for key in keys {
        if !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

fn check_core_fields(core_fields: &Value) -> Result<(), RecordTypeError> {
    let map = core_fields
        .as_object()
        .ok_or_else(|| RecordTypeError::CoreFields("must be an object".into()))?;
    for (field, mode) in map {
        if !CORE_FIELDS.contains(&field.as_str()) {
            return Err(RecordTypeError::CoreFields(format!(
                "`{field}` isn't a shared field"
            )));
        }
        serde_json::from_value::<CoreFieldMode>(mode.clone()).map_err(|_| {
            RecordTypeError::CoreFields(format!("`{field}` must be hidden, optional or required"))
        })?;
    }
    Ok(())
}

/// Outcome keys are keys and unique; an outcome needs a name unless it is
/// one of the built-in's own, whose name is a catalogue string.
fn check_outcomes(
    outcomes: &[RecordOutcome],
    builtin_slug: Option<&str>,
) -> Result<(), RecordTypeError> {
    let catalogue: HashSet<&str> = builtin_slug
        .and_then(record_catalogue::entry)
        .map(|e| e.outcomes.iter().map(|(k, _)| *k).collect())
        .unwrap_or_default();
    let mut seen = HashSet::new();
    for outcome in outcomes {
        if !KEY.is_match(&outcome.key) {
            return Err(RecordTypeError::InvalidKey(outcome.key.clone()));
        }
        if !seen.insert(outcome.key.as_str()) {
            return Err(RecordTypeError::Outcomes(format!(
                "`{}` is listed twice",
                outcome.key
            )));
        }
        let named = outcome
            .label
            .as_deref()
            .is_some_and(|l| !l.trim().is_empty());
        if !named && !catalogue.contains(outcome.key.as_str()) {
            return Err(RecordTypeError::Outcomes(format!(
                "`{}` needs a name",
                outcome.key
            )));
        }
    }
    Ok(())
}

fn check_kinds(conn: &mut DbConnection, kinds: &[String]) -> Result<(), RecordTypeError> {
    let known: HashSet<String> = asset_kinds::table
        .filter(asset_kinds::slug.eq_any(kinds))
        .select(asset_kinds::slug)
        .load::<String>(conn)?
        .into_iter()
        .collect();
    match kinds.iter().find(|k| !known.contains(*k)) {
        Some(unknown) => Err(RecordTypeError::UnknownKind(unknown.clone())),
        None => Ok(()),
    }
}

/// Another live type than `except` already opens with `status`.
fn status_taken(conn: &mut DbConnection, status: &str, except: Option<i32>) -> QueryResult<bool> {
    Ok(opened_by_status(conn, status)?.is_some_and(|t| Some(t.id) != except))
}

fn check_status(
    conn: &mut DbConnection,
    status: &str,
    except: Option<i32>,
) -> Result<(), RecordTypeError> {
    if AssetStatus::from_str_opt(status).is_none() {
        return Err(RecordTypeError::UnknownStatus(status.to_string()));
    }
    if status_taken(conn, status, except)? {
        return Err(RecordTypeError::StatusTaken);
    }
    Ok(())
}

fn outcomes_value(outcomes: &[RecordOutcome]) -> Value {
    serde_json::to_value(outcomes).unwrap_or_else(|_| json!([]))
}

fn clean(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

// sync-audit-only: record types are workspace configuration read over REST, audited by tr_audit_asset_record_types
/// Create a workspace's own type.
pub fn create(
    conn: &mut DbConnection,
    input: NewRecordTypeInput,
) -> Result<AssetRecordType, RecordTypeError> {
    let slug = input.slug.trim().to_string();
    if !KEY.is_match(&slug) {
        return Err(RecordTypeError::InvalidKey(slug));
    }
    if record_catalogue::entry(&slug).is_some() {
        return Err(RecordTypeError::SlugReserved);
    }
    let label = clean(Some(input.label)).ok_or(RecordTypeError::LabelRequired)?;
    let icon = clean(input.icon);
    let color = clean(input.color);
    check_display(Some(&label), icon.as_deref(), color.as_deref())?;
    validate_schema(&input.field_schema).map_err(RecordTypeError::Schema)?;
    check_core_fields(&input.core_fields)?;
    check_outcomes(&input.outcomes, None)?;
    if let Some(kinds) = &input.applies_to_kinds {
        check_kinds(conn, kinds)?;
    }
    let opens_with_status = clean(input.opens_with_status);
    if let Some(status) = &opens_with_status {
        check_status(conn, status, None)?;
    }
    if get_by_slug(conn, &slug)?.is_some() {
        return Err(RecordTypeError::SlugTaken);
    }
    let field_order = normalise_order(&input.field_schema, &input.field_order);
    Ok(diesel::insert_into(asset_record_types::table)
        .values(&NewAssetRecordType {
            slug,
            label: Some(label),
            description: clean(input.description),
            icon,
            color,
            field_schema: input.field_schema,
            field_order,
            core_fields: input.core_fields,
            outcomes: outcomes_value(&input.outcomes),
            applies_to_kinds: input.applies_to_kinds,
            opens_with_status,
            is_builtin: false,
            sort_order: input.sort_order,
            created_by: input.created_by,
        })
        .get_result(conn)?)
}

// sync-audit-only: record types are workspace configuration read over REST, audited by tr_audit_asset_record_types
/// Edit type `id`. A change to its fields or outcomes first checks the
/// type's records against it, and is refused when some wouldn't fit unless
/// `force`.
pub fn update(
    conn: &mut DbConnection,
    id: i32,
    changes: RecordTypeChanges,
    force: bool,
) -> Result<AssetRecordType, RecordTypeError> {
    conn.transaction::<AssetRecordType, RecordTypeError, _>(|conn| {
        let current: AssetRecordType = asset_record_types::table
            .find(id)
            .for_no_key_update()
            .first(conn)
            .optional()?
            .ok_or(RecordTypeError::NotFound)?;
        let label = changes.label.map(clean);
        if !current.is_builtin && matches!(label, Some(None)) {
            return Err(RecordTypeError::LabelRequired);
        }
        let icon = changes.icon.map(clean);
        let color = changes.color.map(clean);
        check_display(
            label.as_ref().and_then(|l| l.as_deref()),
            icon.as_ref().and_then(|i| i.as_deref()),
            color.as_ref().and_then(|c| c.as_deref()),
        )?;
        if let Some(schema) = &changes.field_schema {
            validate_schema(schema).map_err(RecordTypeError::Schema)?;
        }
        if let Some(core_fields) = &changes.core_fields {
            check_core_fields(core_fields)?;
        }
        if let Some(outcomes) = &changes.outcomes {
            let builtin = current.is_builtin.then_some(current.slug.as_str());
            check_outcomes(outcomes, builtin)?;
        }
        if let Some(Some(kinds)) = &changes.applies_to_kinds {
            check_kinds(conn, kinds)?;
        }
        let opens_with_status = changes.opens_with_status.map(clean);
        if let Some(Some(status)) = &opens_with_status {
            check_status(conn, status, Some(id))?;
        }

        let schema = changes
            .field_schema
            .clone()
            .unwrap_or_else(|| current.field_schema.clone());
        if changes.field_schema.is_some() || changes.outcomes.is_some() {
            let outcomes = changes
                .outcomes
                .clone()
                .unwrap_or_else(|| current.outcome_list());
            let (count, samples) = invalid_records(conn, id, &schema, &outcomes, 10)?;
            if count > 0 && !force {
                return Err(RecordTypeError::InvalidRecords { count, samples });
            }
        }
        let field_order = if changes.field_schema.is_some() || changes.field_order.is_some() {
            let order = changes
                .field_order
                .clone()
                .unwrap_or_else(|| current.field_order.iter().flatten().cloned().collect());
            Some(normalise_order(&schema, &order))
        } else {
            None
        };

        Ok(diesel::update(asset_record_types::table.find(id))
            .set(&AssetRecordTypeChange {
                label,
                description: changes.description.map(clean),
                icon,
                color,
                field_schema: changes.field_schema,
                field_order,
                core_fields: changes.core_fields,
                outcomes: changes.outcomes.as_deref().map(outcomes_value),
                applies_to_kinds: changes.applies_to_kinds,
                opens_with_status,
                sort_order: changes.sort_order,
                archived_at: None,
                updated_at: Some(Utc::now()),
            })
            .get_result(conn)?)
    })
}

// sync-audit-only: record types are workspace configuration read over REST, audited by tr_audit_asset_record_types
/// Archive type `id`: its records keep it, and pickers leave it out.
pub fn archive(conn: &mut DbConnection, id: i32) -> Result<AssetRecordType, RecordTypeError> {
    diesel::update(asset_record_types::table.find(id))
        .set((
            asset_record_types::archived_at.eq(Some(Utc::now())),
            asset_record_types::updated_at.eq(Utc::now()),
        ))
        .get_result(conn)
        .optional()?
        .ok_or(RecordTypeError::NotFound)
}

// sync-audit-only: record types are workspace configuration read over REST, audited by tr_audit_asset_record_types
/// Bring archived type `id` back. Refused when another live type opens with
/// its status meanwhile.
pub fn restore(conn: &mut DbConnection, id: i32) -> Result<AssetRecordType, RecordTypeError> {
    let current = get(conn, id)?.ok_or(RecordTypeError::NotFound)?;
    if let Some(status) = &current.opens_with_status {
        if current.archived_at.is_some() && status_taken(conn, status, Some(id))? {
            return Err(RecordTypeError::StatusTaken);
        }
    }
    Ok(diesel::update(asset_record_types::table.find(id))
        .set((
            asset_record_types::archived_at.eq(None::<chrono::DateTime<Utc>>),
            asset_record_types::updated_at.eq(Utc::now()),
        ))
        .get_result(conn)?)
}

// sync-audit-only: record types are workspace configuration read over REST, audited by tr_audit_asset_record_types
/// Delete type `id`: only a workspace's own type that no record uses.
pub fn delete(conn: &mut DbConnection, id: i32) -> Result<(), RecordTypeError> {
    let current = get(conn, id)?.ok_or(RecordTypeError::NotFound)?;
    if current.is_builtin {
        return Err(RecordTypeError::Builtin);
    }
    let count = count_records(conn, id)?;
    if count > 0 {
        return Err(RecordTypeError::InUse { count });
    }
    diesel::delete(asset_record_types::table.find(id)).execute(conn)?;
    Ok(())
}

// sync-audit-only: record types are workspace configuration read over REST, audited by tr_audit_asset_record_types
/// Add catalogue entry `slug` to the workspace, or give back the type it
/// added before, as it stands now (renamed, edited or archived). The type is
/// built-in with no name of its own, so it reads in each person's language.
/// It opens with its status only when no other live type does already.
pub fn add_from_catalogue(
    conn: &mut DbConnection,
    slug: &str,
    created_by: Option<Uuid>,
) -> Result<AssetRecordType, RecordTypeError> {
    let entry = record_catalogue::entry(slug).ok_or(RecordTypeError::NotInCatalogue)?;
    if let Some(existing) = get_by_slug(conn, slug)? {
        return Ok(existing);
    }
    let opens_with_status = match entry.opens_with_status.map(AssetStatus::as_str) {
        Some(status) if !status_taken(conn, status, None)? => Some(status.to_string()),
        _ => None,
    };
    diesel::insert_into(asset_record_types::table)
        .values(&NewAssetRecordType {
            slug: entry.slug.to_string(),
            label: None,
            description: None,
            icon: Some(entry.icon.to_string()),
            color: None,
            field_schema: entry.field_schema(),
            field_order: entry.field_order(),
            core_fields: entry.core_fields(),
            outcomes: entry.outcomes(),
            applies_to_kinds: None,
            opens_with_status,
            is_builtin: true,
            sort_order: entry.sort_order,
            created_by,
        })
        .on_conflict((asset_record_types::workspace_id, asset_record_types::slug))
        .do_nothing()
        .execute(conn)?;
    get_by_slug(conn, slug)?.ok_or(RecordTypeError::NotFound)
}
